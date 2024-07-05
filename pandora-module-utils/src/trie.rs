// Copyright 2024 Wladimir Palant
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This implements a specialized prefix tree (trie) data structure. The design goal are:
//!
//! * Memory-efficient data storage after the setup phase
//! * Zero allocation and copying during lookup
//! * Efficient lookup
//! * The labels are segmented with a separator character (forward slash) and only full segment
//!   matches are accepted.
//! * Different value returned for exact and prefix matches
//! * When the same value is used multiple times, only one copy is stored

use std::{
    fmt::Debug,
    ops::{Deref, Range},
};

/// Character to separate labels
pub(crate) const SEPARATOR: u8 = b'/';

/// Calculates the length of the longest common prefix of two labels. A common prefix is identical
/// and ends at a boundary in both labels (either end of the label or a separator character).
pub(crate) fn common_prefix_length(a: &[u8], b: &[u8]) -> usize {
    let mut length = 0;
    for i in 0..std::cmp::min(a.len(), b.len()) {
        if a[i] != b[i] {
            return length;
        }

        if a[i] == SEPARATOR {
            length = i;
        }
    }

    if a.len() == b.len() || (a.len() < b.len() && b[a.len()] == SEPARATOR) {
        // exact match or A is a prefix of B
        length = a.len();
    } else if a.len() > b.len() && a[b.len()] == SEPARATOR {
        // B is a prefix of A
        length = b.len();
    }
    length
}

/// A trie data structure
///
/// To use memory more efficiently and to improve locality, this stores all data in three vectors.
/// One lists all nodes, ordered in such a way that children of one node are always stored
/// consecutively and sorted by their label. A node stores an index range referring to its
/// children.
///
/// Since values are optional and potentially rather large, existing values are stored in a
/// separate vector. The node stores an optional index of its value, not the value itself.
///
/// Finally, the third vector stores the labels of the nodes, so that nodes don’t need separate
/// allocations for their labels. Each nodes refers to its label within this vector via an index
/// range.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Trie<Value> {
    nodes: Vec<Node>,
    values: Vec<Value>,
    labels: Vec<u8>,
}

/// Trie lookup result, will dereference into the value
#[derive(Debug, Clone)]
pub struct LookupResult<'a, Value> {
    value: &'a Value,
    index: usize,
}

impl<'a, Value> LookupResult<'a, Value> {
    fn new(value: &'a Value, index: usize) -> Self {
        Self { value, index }
    }

    /// The index of the referenced value, allows retrieving it again without going through another
    /// lookup.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Retrieves the inner value
    ///
    /// Unlike dereferencing, this propagates lifetimes properly
    pub fn as_value(&self) -> &'a Value {
        self.value
    }
}

impl<Value> Deref for LookupResult<'_, Value> {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

/// A trie node
///
/// A node label can consist of one or multiple segments (separated by `SEPARATOR`). These segments
/// represent the route to the node from its parent node.
///
/// The value is optional. Nodes without a value serve merely as a routing point for multiple child
/// nodes.
///
/// Each child node represents a unique path further from this node. Multiple child node labels
/// never start with the same segment: in such scenarios the builder inserts an intermediate node
/// that serves as the common parent for all nodes reachable via that segment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    label: Range<usize>,
    value_exact: Option<usize>,
    value_prefix: Option<usize>,
    children: Range<usize>,
}

impl<Value> Trie<Value> {
    /// Index of the root node in the `nodes` vector, this is where lookup always starts.
    const ROOT: usize = 0;

    /// Returns a builder instance that can be used to set up the trie.
    pub(crate) fn builder() -> TrieBuilder<Value>
    where
        Value: Eq,
    {
        TrieBuilder::<Value>::new()
    }

    /// Converts a value index into a lookup result
    fn to_lookup_result(&self, result: Option<usize>) -> Option<LookupResult<'_, Value>> {
        result
            .and_then(|index| Some((self.values.get(index)?, index)))
            .map(|(value, index)| (LookupResult::new(value, index)))
    }

    /// Looks up a particular label in the trie.
    ///
    /// The label is identified by an iterator producing segments. The segments are expected to be
    /// normalized: no empty segments exist and no segments contain the separator character.
    ///
    /// This will return the value corresponding to the longest matching path if any.
    pub(crate) fn lookup<'a, L>(&self, mut label: L) -> Option<LookupResult<'_, Value>>
    where
        L: Iterator<Item = &'a [u8]>,
    {
        let mut result_exact;
        let mut result_prefix = None;
        let mut current = self.nodes.get(Self::ROOT)?;
        loop {
            result_exact = current.value_exact;
            if current.value_prefix.is_some() {
                result_prefix = current.value_prefix;
            }

            let segment = if let Some(segment) = label.next() {
                segment
            } else {
                // End of label, return either exact or prefix result
                return self.to_lookup_result(result_exact.or(result_prefix));
            };

            // TODO: Binary search might be more efficient here
            let mut found_match = false;
            for child in current.children.start..current.children.end {
                let child = self.nodes.get(child)?;
                let mut label_start = child.label.start;
                let label_end = child.label.end;
                let length = common_prefix_length(segment, &self.labels[label_start..label_end]);
                if length > 0 {
                    label_start += length;

                    // Keep matching more segments until there is no more label left
                    while label_end > label_start {
                        // Skip separator character
                        label_start += 1;

                        let segment = if let Some(segment) = label.next() {
                            segment
                        } else {
                            // End of label, return whatever we’ve got
                            return self.to_lookup_result(result_prefix);
                        };

                        let length =
                            common_prefix_length(segment, &self.labels[label_start..label_end]);
                        if length > 0 {
                            label_start += length;
                        } else {
                            // Got only a partial match
                            return self.to_lookup_result(result_prefix);
                        }
                    }

                    found_match = true;
                    current = child;
                    break;
                }
            }

            if !found_match {
                return self.to_lookup_result(result_prefix);
            }
        }
    }

    /// Retrieves the value from a previous lookup by its index
    pub(crate) fn retrieve(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    fn fmt_field(
        &self,
        f: &mut std::fmt::DebugStruct<'_, '_>,
        index: usize,
        prefix: &[u8],
    ) -> std::fmt::Result
    where
        Value: Debug,
    {
        let node = &self.nodes[index];
        let mut label = prefix.to_vec();
        label.extend_from_slice(&self.labels[node.label.start..node.label.end]);
        if node.value_exact.is_some() || node.value_prefix.is_some() {
            // Fields are considered dead code here because they are only ever read by the Debug
            // implementation.
            #[allow(dead_code)]
            #[derive(Debug)]
            struct Node<'a, Value: Debug> {
                value_exact: Option<&'a Value>,
                value_prefix: Option<&'a Value>,
            }

            let value = Node {
                value_exact: node.value_exact.map(|index| &self.values[index]),
                value_prefix: node.value_prefix.map(|index| &self.values[index]),
            };

            f.field(&String::from_utf8_lossy(&label), &value);
        }

        label.push(SEPARATOR);
        for child in node.children.start..node.children.end {
            self.fmt_field(f, child, &label)?;
        }

        Ok(())
    }
}

impl<Value> Debug for Trie<Value>
where
    Value: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("Trie");
        self.fmt_field(&mut f, Self::ROOT, b"")
    }
}

/// A trie builder used to set up a `Trie` instance
///
/// In addition to setting up the trie structure, this will keep track of the requires allocation
/// size for the trie vectors.
#[derive(Debug)]
pub(crate) struct TrieBuilder<Value> {
    nodes: usize,
    labels: usize,
    root: BuilderNode<Value>,
}

/// A builder node
///
/// Unlike `Node` this data structure references its label, children and value directly.
#[derive(Debug)]
struct BuilderNode<Value> {
    label: Vec<u8>,
    children: Vec<BuilderNode<Value>>,
    value_exact: Option<Value>,
    value_prefix: Option<Value>,
}

impl<Value: Eq> TrieBuilder<Value> {
    /// Creates a new builder.
    fn new() -> Self {
        Self {
            nodes: 1,
            labels: 0,
            root: BuilderNode::<Value> {
                label: Vec::new(),
                children: Vec::new(),
                value_exact: None,
                value_prefix: None,
            },
        }
    }

    /// Recursively finds the node that a particular label should be added to.
    ///
    /// If the label shares a common prefix with a child node of the current node, this will
    /// insert a new intermediate node if necessary (new parent for both than child node and the
    /// node to be added) and recurses. As it recurses, it will trim down the label accordingly to
    /// the path already traveled.
    ///
    /// If no nodes with common prefixes are found, then the current node is the one that the
    /// label should be added to.
    fn find_insertion_point<'a>(
        current: &'a mut BuilderNode<Value>,
        nodes: &mut usize,
        labels: &mut usize,
        label: &mut Vec<u8>,
    ) -> &'a mut BuilderNode<Value> {
        let mut match_ = None;
        for (i, node) in current.children.iter_mut().enumerate() {
            let length = common_prefix_length(&node.label, label);
            if length > 0 {
                label.drain(..std::cmp::min(length + 1, label.len()));
                if length < node.label.len() {
                    // Partial match, insert a new node and make the original its child
                    let mut head: Vec<_> = node.label.drain(..length + 1).collect();

                    // Remove separator
                    head.pop();

                    *nodes += 1;

                    // Splitting the node label in two results in one character less (separator)
                    *labels -= 1;

                    let mut new_node = BuilderNode {
                        label: head,
                        children: Vec::new(),
                        value_exact: None,
                        value_prefix: None,
                    };

                    std::mem::swap(node, &mut new_node);
                    node.children.push(new_node);
                };

                match_ = Some(i);
                break;
            }
        }

        return match match_ {
            Some(i) => Self::find_insertion_point(&mut current.children[i], nodes, labels, label),
            None => current,
        };
    }

    /// Adds a value for the given label. Will return `true` if an existing value was overwritten.
    ///
    /// `value_exact` will only be returned for exact matches. If present, `value_prefix` will be
    /// returned for any paths starting with the given label.
    ///
    /// The label is expected to be normalized: no separator characters at the beginning or end, and
    /// always only one separator character used to separate segments.
    pub(crate) fn push(
        &mut self,
        mut label: Vec<u8>,
        value_exact: Value,
        value_prefix: Option<Value>,
    ) -> bool {
        let node = Self::find_insertion_point(
            &mut self.root,
            &mut self.nodes,
            &mut self.labels,
            &mut label,
        );

        if label.is_empty() {
            // Exact match, replace the value for this node
            let had_value = node.value_exact.is_some();
            node.value_exact = Some(value_exact);
            node.value_prefix = value_prefix;
            had_value
        } else {
            // Insert new node as child of the current one
            self.nodes += 1;
            self.labels += label.len();
            node.children.push(BuilderNode {
                label,
                children: Vec::new(),
                value_exact: Some(value_exact),
                value_prefix,
            });
            false
        }
    }

    /// Pushes an empty entry into the nodes vector.
    ///
    /// This is used to allocate space for the node, so that child nodes are always stored
    /// consecutively. The values are adjusted by `into_trie_node` later.
    fn push_trie_node(nodes: &mut Vec<Node>) {
        nodes.push(Node {
            label: 0..0,
            value_exact: None,
            value_prefix: None,
            children: 0..0,
        });
    }

    /// Returns the index of an already existing value entry or adds a new entry to the collection
    /// and returns its index.
    fn add_value(value: Value, values: &mut Vec<Value>) -> usize {
        if let Some(index) = values.iter().position(|v| v == &value) {
            index
        } else {
            let index = values.len();
            values.push(value);
            index
        }
    }

    /// Sets up an entry in the nodes vector.
    ///
    /// This will transfer data from a builder node to the trie node identified via index. It will
    /// also recurse to make sure child nodes of the current node are transferred as well.
    fn into_trie_node(
        mut current: BuilderNode<Value>,
        index: usize,
        nodes: &mut Vec<Node>,
        labels: &mut Vec<u8>,
        values: &mut Vec<Value>,
    ) {
        nodes[index].label = labels.len()..labels.len() + current.label.len();
        labels.append(&mut current.label);

        if let Some(value) = current.value_exact {
            nodes[index].value_exact = Some(Self::add_value(value, values));
        }
        if let Some(value) = current.value_prefix {
            nodes[index].value_prefix = Some(Self::add_value(value, values));
        }

        current.children.sort_by(|a, b| a.label.cmp(&b.label));

        let mut child_index = nodes.len();
        nodes[index].children = child_index..child_index + current.children.len();
        for _ in &current.children {
            Self::push_trie_node(nodes);
        }

        for child in current.children {
            Self::into_trie_node(child, child_index, nodes, labels, values);
            child_index += 1;
        }
    }

    /// Translates the builder data into a `Trie` instance.
    pub(crate) fn build(self) -> Trie<Value> {
        let mut nodes = Vec::with_capacity(self.nodes);
        let mut labels = Vec::with_capacity(self.labels);
        let mut values = Vec::new();

        let index = nodes.len();
        Self::push_trie_node(&mut nodes);
        Self::into_trie_node(self.root, index, &mut nodes, &mut labels, &mut values);

        assert_eq!(nodes.len(), self.nodes);
        assert_eq!(labels.len(), self.labels);
        values.shrink_to_fit();

        Trie {
            nodes,
            labels,
            values,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key<'a>(s: &'a str) -> Box<dyn Iterator<Item = &[u8]> + 'a> {
        Box::new(
            s.as_bytes()
                .split(|c| *c == SEPARATOR)
                .filter(|s| !s.is_empty()),
        )
    }

    #[test]
    fn common_prefix() {
        assert_eq!(common_prefix_length(b"", b""), 0);
        assert_eq!(common_prefix_length(b"abc", b""), 0);
        assert_eq!(common_prefix_length(b"", b"abc"), 0);
        assert_eq!(common_prefix_length(b"abc", b"abc"), 3);
        assert_eq!(common_prefix_length(b"a", b"abc"), 0);
        assert_eq!(common_prefix_length(b"abc", b"a"), 0);
        assert_eq!(common_prefix_length(b"a", b"a/bc"), 1);
        assert_eq!(common_prefix_length(b"a/bc", b"a"), 1);
        assert_eq!(common_prefix_length(b"a/b", b"a/bc"), 1);
        assert_eq!(common_prefix_length(b"a/bc", b"a/b"), 1);
        assert_eq!(common_prefix_length(b"a/bc", b"a/bc"), 4);
        assert_eq!(common_prefix_length(b"a/bc", b"a/bc/d"), 4);
        assert_eq!(common_prefix_length(b"a/bc/d", b"a/bc"), 4);
        assert_eq!(common_prefix_length(b"a/bc/d", b"x/bc/d"), 0);
    }

    #[test]
    fn lookup_with_root_value() {
        let mut builder = Trie::builder();
        for (label, value_exact, value_prefix) in [
            ("", 1, 11),
            ("a", 2, 12),
            ("bc", 7, 17),
            ("a/bc/de/f", 3, 13),
            ("a/bc", 4, 14),
            ("a/bc/de/g", 5, 15),
        ] {
            assert!(!builder.push(label.as_bytes().to_vec(), value_exact, Some(value_prefix)));
        }
        assert!(builder.push("a/bc".as_bytes().to_vec(), 6, Some(16)));
        let trie = builder.build();

        assert_eq!(trie.lookup(make_key("")).as_deref(), Some(&1));
        assert_eq!(trie.lookup(make_key("a")).as_deref(), Some(&2));
        assert_eq!(trie.lookup(make_key("x")).as_deref(), Some(&11));
        assert_eq!(trie.lookup(make_key("bc")).as_deref(), Some(&7));
        assert_eq!(trie.lookup(make_key("x/y")).as_deref(), Some(&11));
        assert_eq!(trie.lookup(make_key("a/bc")).as_deref(), Some(&6));
        assert_eq!(trie.lookup(make_key("a/b")).as_deref(), Some(&12));
        assert_eq!(trie.lookup(make_key("a/bcde")).as_deref(), Some(&12));
        assert_eq!(trie.lookup(make_key("a/bc/de")).as_deref(), Some(&16));
        assert_eq!(trie.lookup(make_key("a/bc/de/f")).as_deref(), Some(&3));
        assert_eq!(trie.lookup(make_key("a/bc/de/fh")).as_deref(), Some(&16));
        assert_eq!(trie.lookup(make_key("a/bc/de/g")).as_deref(), Some(&5));
        assert_eq!(trie.lookup(make_key("a/bc/de/h")).as_deref(), Some(&16));
    }

    #[test]
    fn lookup_without_root_value() {
        let mut builder = Trie::builder();
        for (label, value_exact, value_prefix) in [
            ("a", 2, 12),
            ("bc", 7, 17),
            ("a/bc/de/f", 3, 13),
            ("a/bc", 4, 14),
            ("a/bc/de/g", 5, 15),
        ] {
            assert!(!builder.push(label.as_bytes().to_vec(), value_exact, Some(value_prefix)));
        }
        assert!(builder.push("a/bc".as_bytes().to_vec(), 6, Some(16)));
        let trie = builder.build();

        assert_eq!(trie.lookup(make_key("")).as_deref(), None);
        assert_eq!(trie.lookup(make_key("a")).as_deref(), Some(&2));
        assert_eq!(trie.lookup(make_key("x")).as_deref(), None);
        assert_eq!(trie.lookup(make_key("b")).as_deref(), None);
        assert_eq!(trie.lookup(make_key("bc")).as_deref(), Some(&7));
        assert_eq!(trie.lookup(make_key("bcd")).as_deref(), None);
        assert_eq!(trie.lookup(make_key("x/y")).as_deref(), None);
        assert_eq!(trie.lookup(make_key("a/bc")).as_deref(), Some(&6));
        assert_eq!(trie.lookup(make_key("a/b")).as_deref(), Some(&12));
        assert_eq!(trie.lookup(make_key("a/bcde")).as_deref(), Some(&12));
        assert_eq!(trie.lookup(make_key("a/bc/de")).as_deref(), Some(&16));
        assert_eq!(trie.lookup(make_key("a/bc/de/f")).as_deref(), Some(&3));
        assert_eq!(trie.lookup(make_key("a/bc/de/fh")).as_deref(), Some(&16));
        assert_eq!(trie.lookup(make_key("a/bc/de/g")).as_deref(), Some(&5));
        assert_eq!(trie.lookup(make_key("a/bc/de/h")).as_deref(), Some(&16));
    }

    #[test]
    fn value_compacting() {
        let mut builder = Trie::builder();
        for (label, value_exact, value_prefix) in [
            ("a", 123, 123),
            ("bc", 123, 123),
            ("a/bc/de/f", 123, 456),
            ("a/bc", 123, 123),
            ("a/bc/de/g", 123, 123),
        ] {
            assert!(!builder.push(label.as_bytes().to_vec(), value_exact, Some(value_prefix)));
        }
        let trie = builder.build();
        assert_eq!(trie.values.len(), 2);
    }
}
