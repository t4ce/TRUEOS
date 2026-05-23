use crate::data::Create;
use alloc::boxed::Box;
use core::convert::TryFrom;
use core::error::Error;
use core::fmt::{Display, Formatter};
use dot_parser::ast::AList;
use dot_parser::ast::Graph as DotGraph;
use dot_parser::ast::PestError as ParsingError;
use dot_parser::canonical::Graph as CGraph;
use dot_parser::canonical::Node;

pub type DotNodeWeight<'a> = Node<(&'a str, &'a str)>;
pub type DotAttrList<'a> = AList<(&'a str, &'a str)>;

#[derive(Debug)]
pub struct DotParsingError {
    error: Box<ParsingError>,
}

impl Display for DotParsingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "{}", self.error)
    }
}

impl From<ParsingError> for DotParsingError {
    fn from(error: ParsingError) -> Self {
        Self {
            error: Box::new(error),
        }
    }
}

impl Error for DotParsingError {}

/// This trait extends [Create] with a method to parse a graph from a dot string.
pub trait ParseFromDot<'a>:
    Create<EdgeWeight = DotAttrList<'a>, NodeWeight = DotNodeWeight<'a>>
{
    /// Convert a DOT/Graphviz graph (represented as an [DotGraph]) into a petgraph's graph.
    fn from_dot_graph(dot_graph: DotGraph<(&'a str, &'a str)>) -> Self {
        let dot_graph: CGraph<(&'a str, &'a str)> = dot_graph.into();
        let node_number = dot_graph.nodes.set.len();
        let edge_number = dot_graph.edges.set.len();
        let mut graph = Self::with_capacity(node_number, edge_number);
        let mut node_indices = std::collections::HashMap::new();
        for node in dot_graph.nodes.set {
            let ni = graph.add_node(node.1);
            node_indices.insert(node.0, ni);
        }
        for edge in dot_graph.edges.set {
            let from_ni = node_indices.get(&edge.from).unwrap();
            let to_ni = node_indices.get(&edge.to).unwrap();
            graph.add_edge(*from_ni, *to_ni, edge.attr);
        }
        graph
    }

    /// Attempt to parse a DOT/Graphviz string into a graph. Fail if the string is not a
    /// well-formed DOT/Graphviz string.
    fn try_from(s: &'a str) -> Result<Self, DotParsingError> {
        let ast = DotGraph::try_from(s)?;
        let petgraph = Self::from_dot_graph(ast);
        Ok(petgraph)
    }
}

#[macro_export]
/// Statically imports a Graph from a valid DOT/Graphviz [&str].
macro_rules! graph_from_str {
    ($s:tt) => {
        $crate::dot::dot_parser::ParseFromDot::from_dot_graph(dot_parser_macros::from_dot_string!(
            $s
        ))
    };
}

#[macro_export]
/// Statically imports a Graph from a DOT/Graphviz file. The macro expects the file path as argument.
///
/// Notice that, since the graph is imported *statically*, the file must exist at compile time, but
/// can be removed at runtime.
macro_rules! graph_from_file {
    ($s:tt) => {
        $crate::dot::dot_parser::ParseFromDot::from_dot_graph(dot_parser_macros::from_dot_file!($s))
    };
}

pub use graph_from_file;
pub use graph_from_str;
