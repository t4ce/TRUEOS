use crate::visit::{
    Data, EdgeRef, GraphBase, GraphProp, GraphRef, IntoEdgeReferences, IntoEdges,
    IntoEdgesDirected, IntoNeighbors, IntoNeighborsDirected, IntoNodeIdentifiers,
    IntoNodeReferences, NodeCompactIndexable, NodeCount, NodeIndexable, Visitable,
};
use crate::Direction;

/// An edge direction removing graph adaptor.
#[derive(Copy, Clone, Debug)]
pub struct UndirectedAdaptor<G>(pub G);

impl<G: GraphRef> GraphRef for UndirectedAdaptor<G> {}

impl<G> IntoNeighbors for UndirectedAdaptor<G>
where
    G: IntoNeighborsDirected,
{
    type Neighbors = core::iter::Chain<G::NeighborsDirected, G::NeighborsDirected>;
    fn neighbors(self, n: G::NodeId) -> Self::Neighbors {
        self.0
            .neighbors_directed(n, Direction::Incoming)
            .chain(self.0.neighbors_directed(n, Direction::Outgoing))
    }
}

impl<G> IntoEdges for UndirectedAdaptor<G>
where
    G: IntoEdgesDirected,
{
    type Edges = core::iter::Chain<
        MaybeReversedEdges<G::EdgesDirected>,
        MaybeReversedEdges<G::EdgesDirected>,
    >;
    fn edges(self, a: Self::NodeId) -> Self::Edges {
        let incoming = MaybeReversedEdges {
            iter: self.0.edges_directed(a, Direction::Incoming),
            reversed: true,
        };
        let outgoing = MaybeReversedEdges {
            iter: self.0.edges_directed(a, Direction::Outgoing),
            reversed: false,
        };
        incoming.chain(outgoing)
    }
}

impl<G> GraphProp for UndirectedAdaptor<G>
where
    G: GraphBase,
{
    type EdgeType = crate::Undirected;

    fn is_directed(&self) -> bool {
        false
    }
}

/// An edges iterator which may reverse the edge orientation.
#[derive(Debug, Clone)]
pub struct MaybeReversedEdges<I> {
    iter: I,
    reversed: bool,
}

impl<I> Iterator for MaybeReversedEdges<I>
where
    I: Iterator,
    I::Item: EdgeRef,
{
    type Item = MaybeReversedEdgeReference<I::Item>;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|x| MaybeReversedEdgeReference {
            inner: x,
            reversed: self.reversed,
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

/// An edge reference which may reverse the edge orientation.
#[derive(Copy, Clone, Debug)]
pub struct MaybeReversedEdgeReference<R> {
    inner: R,
    reversed: bool,
}

impl<R> EdgeRef for MaybeReversedEdgeReference<R>
where
    R: EdgeRef,
{
    type NodeId = R::NodeId;
    type EdgeId = R::EdgeId;
    type Weight = R::Weight;
    fn source(&self) -> Self::NodeId {
        if self.reversed {
            self.inner.target()
        } else {
            self.inner.source()
        }
    }
    fn target(&self) -> Self::NodeId {
        if self.reversed {
            self.inner.source()
        } else {
            self.inner.target()
        }
    }
    fn weight(&self) -> &Self::Weight {
        self.inner.weight()
    }
    fn id(&self) -> Self::EdgeId {
        self.inner.id()
    }
}

/// An edges iterator which may reverse the edge orientation.
#[derive(Debug, Clone)]
pub struct MaybeReversedEdgeReferences<I> {
    iter: I,
}

impl<I> Iterator for MaybeReversedEdgeReferences<I>
where
    I: Iterator,
    I::Item: EdgeRef,
{
    type Item = MaybeReversedEdgeReference<I::Item>;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|x| MaybeReversedEdgeReference {
            inner: x,
            reversed: false,
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<G> IntoEdgeReferences for UndirectedAdaptor<G>
where
    G: IntoEdgeReferences,
{
    type EdgeRef = MaybeReversedEdgeReference<G::EdgeRef>;
    type EdgeReferences = MaybeReversedEdgeReferences<G::EdgeReferences>;

    fn edge_references(self) -> Self::EdgeReferences {
        MaybeReversedEdgeReferences {
            iter: self.0.edge_references(),
        }
    }
}

macro_rules! access0 {
    ($e:expr) => {
        $e.0
    };
}

GraphBase! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
Data! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
Visitable! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
NodeIndexable! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
NodeCompactIndexable! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
IntoNodeIdentifiers! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
IntoNodeReferences! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
NodeCount! {delegate_impl [[G], G, UndirectedAdaptor<G>, access0]}
