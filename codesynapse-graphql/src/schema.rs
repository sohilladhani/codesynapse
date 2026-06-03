use std::sync::{Arc, RwLock};

use async_graphql::{Context, EmptySubscription, Object, Schema, SimpleObject};

use crate::state::{GqlEdge, GqlNode, GraphState};

pub type AppSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

pub fn build_schema(state: Arc<RwLock<GraphState>>) -> AppSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(state)
        .finish()
}

fn state<'a>(ctx: &'a Context<'a>) -> std::sync::RwLockReadGuard<'a, GraphState> {
    ctx.data_unchecked::<Arc<RwLock<GraphState>>>()
        .read()
        .unwrap()
}

fn state_mut<'a>(ctx: &'a Context<'a>) -> std::sync::RwLockWriteGuard<'a, GraphState> {
    ctx.data_unchecked::<Arc<RwLock<GraphState>>>()
        .write()
        .unwrap()
}

#[derive(Clone, SimpleObject)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub source_location: Option<String>,
    pub community: Option<i64>,
    pub file_type: String,
    pub rationale: Option<String>,
}

#[derive(Clone, SimpleObject)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
    pub source_file: Option<String>,
    pub weight: f64,
    pub context: Option<String>,
}

#[derive(Clone, SimpleObject)]
pub struct GraphStats {
    pub node_count: i32,
    pub edge_count: i32,
    pub community_count: i32,
}

fn node_from(n: GqlNode) -> Node {
    Node {
        id: n.id,
        label: n.label,
        source_file: n.source_file,
        source_location: n.source_location,
        community: n.community,
        file_type: n.file_type,
        rationale: n.rationale,
    }
}

fn edge_from(e: GqlEdge) -> Edge {
    Edge {
        source: e.source,
        target: e.target,
        relation: e.relation,
        confidence: e.confidence,
        source_file: e.source_file,
        weight: e.weight,
        context: e.context,
    }
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn node(&self, ctx: &Context<'_>, id: String) -> Option<Node> {
        state(ctx).get_node(&id).cloned().map(node_from)
    }

    async fn nodes(&self, ctx: &Context<'_>, limit: Option<i32>) -> Vec<Node> {
        let s = state(ctx);
        let all = s.get_nodes();
        let cap = limit.map(|l| l as usize).unwrap_or(all.len());
        all.into_iter().take(cap).map(node_from).collect()
    }

    async fn neighbors(&self, ctx: &Context<'_>, node_id: String, depth: Option<i32>) -> Vec<Node> {
        let d = depth.unwrap_or(1) as usize;
        state(ctx)
            .neighbors(&node_id, d)
            .into_iter()
            .map(node_from)
            .collect()
    }

    async fn community(&self, ctx: &Context<'_>, id: i64) -> Vec<Node> {
        state(ctx)
            .community_nodes(id)
            .into_iter()
            .map(node_from)
            .collect()
    }

    async fn shortest_path(
        &self,
        ctx: &Context<'_>,
        source: String,
        target: String,
    ) -> Vec<String> {
        state(ctx)
            .shortest_path(&source, &target)
            .unwrap_or_default()
    }

    async fn search(&self, ctx: &Context<'_>, query: String, limit: Option<i32>) -> Vec<Node> {
        let cap = limit.unwrap_or(20) as usize;
        state(ctx)
            .search_nodes(&query, cap)
            .into_iter()
            .map(node_from)
            .collect()
    }

    async fn stats(&self, ctx: &Context<'_>) -> GraphStats {
        let s = state(ctx);
        GraphStats {
            node_count: s.node_count() as i32,
            edge_count: s.edge_count() as i32,
            community_count: s.community_count() as i32,
        }
    }

    async fn god_nodes(&self, ctx: &Context<'_>, top_n: Option<i32>) -> Vec<Node> {
        let n = top_n.unwrap_or(10) as usize;
        state(ctx).god_nodes(n).into_iter().map(node_from).collect()
    }

    async fn edges(&self, ctx: &Context<'_>) -> Vec<Edge> {
        state(ctx).get_edges().into_iter().map(edge_from).collect()
    }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    async fn add_node(
        &self,
        ctx: &Context<'_>,
        id: String,
        label: String,
        source_file: String,
        file_type: Option<String>,
    ) -> Node {
        let node = GqlNode {
            id: id.clone(),
            label: label.clone(),
            source_file: source_file.clone(),
            source_location: None,
            community: None,
            file_type: file_type.unwrap_or_else(|| "Code".to_string()),
            rationale: None,
        };
        state_mut(ctx).add_node(node.clone());
        node_from(node)
    }

    async fn add_edge(
        &self,
        ctx: &Context<'_>,
        source: String,
        target: String,
        relation: String,
        confidence: Option<String>,
    ) -> Edge {
        let edge = GqlEdge {
            source: source.clone(),
            target: target.clone(),
            relation: relation.clone(),
            confidence: confidence.unwrap_or_else(|| "EXTRACTED".to_string()),
            source_file: None,
            weight: 1.0,
            context: None,
        };
        state_mut(ctx).add_edge(edge.clone());
        edge_from(edge)
    }

    async fn reset(&self, ctx: &Context<'_>) -> bool {
        state_mut(ctx).reset();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_schema() -> AppSchema {
        build_schema(Arc::new(RwLock::new(GraphState::new())))
    }

    fn schema_with(nodes: &[(&str, &str)], edges: &[(&str, &str, &str)]) -> AppSchema {
        let state = Arc::new(RwLock::new(GraphState::new()));
        {
            let mut s = state.write().unwrap();
            for (id, label) in nodes {
                s.add_node(GqlNode {
                    id: id.to_string(),
                    label: label.to_string(),
                    source_file: "test.rs".to_string(),
                    source_location: None,
                    community: None,
                    file_type: "Code".to_string(),
                    rationale: None,
                });
            }
            for (src, tgt, rel) in edges {
                s.add_edge(GqlEdge {
                    source: src.to_string(),
                    target: tgt.to_string(),
                    relation: rel.to_string(),
                    confidence: "EXTRACTED".to_string(),
                    source_file: None,
                    weight: 1.0,
                    context: None,
                });
            }
        }
        Schema::build(QueryRoot, MutationRoot, EmptySubscription)
            .data(state)
            .finish()
    }

    #[tokio::test]
    async fn test_query_node_found() {
        let schema = schema_with(&[("n1", "AuthService")], &[]);
        let res = schema.execute(r#"{ node(id: "n1") { id label } }"#).await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["node"]["id"], "n1");
        assert_eq!(data["node"]["label"], "AuthService");
    }

    #[tokio::test]
    async fn test_query_node_not_found() {
        let schema = make_schema();
        let res = schema.execute(r#"{ node(id: "missing") { id } }"#).await;
        assert!(res.errors.is_empty());
        assert!(res.data.into_json().unwrap()["node"].is_null());
    }

    #[tokio::test]
    async fn test_query_nodes_empty() {
        let schema = make_schema();
        let res = schema.execute("{ nodes { id } }").await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["nodes"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_query_nodes_all() {
        let schema = schema_with(&[("a", "A"), ("b", "B"), ("c", "C")], &[]);
        let res = schema.execute("{ nodes { id } }").await;
        assert!(res.errors.is_empty());
        let arr = res.data.into_json().unwrap();
        assert_eq!(arr["nodes"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_query_nodes_limit() {
        let schema = schema_with(&[("a", "A"), ("b", "B"), ("c", "C")], &[]);
        let res = schema.execute("{ nodes(limit: 2) { id } }").await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["nodes"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_query_neighbors_direct() {
        let schema = schema_with(&[("a", "A"), ("b", "B")], &[("a", "b", "calls")]);
        let res = schema.execute(r#"{ neighbors(nodeId: "a") { id } }"#).await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let ids: Vec<_> = data["neighbors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"b"));
    }

    #[tokio::test]
    async fn test_query_neighbors_depth2() {
        let schema = schema_with(
            &[("a", "A"), ("b", "B"), ("c", "C")],
            &[("a", "b", "calls"), ("b", "c", "calls")],
        );
        let res = schema
            .execute(r#"{ neighbors(nodeId: "a", depth: 2) { id } }"#)
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let ids: Vec<_> = data["neighbors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"b"));
        assert!(ids.contains(&"c"));
    }

    #[tokio::test]
    async fn test_query_neighbors_unknown_node() {
        let schema = make_schema();
        let res = schema
            .execute(r#"{ neighbors(nodeId: "ghost") { id } }"#)
            .await;
        assert!(res.errors.is_empty());
        assert!(res.data.into_json().unwrap()["neighbors"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_query_community() {
        let state = Arc::new(RwLock::new(GraphState::new()));
        {
            let mut s = state.write().unwrap();
            s.add_node(GqlNode {
                id: "x".into(),
                label: "X".into(),
                source_file: "f.rs".into(),
                source_location: None,
                community: Some(1),
                file_type: "Code".into(),
                rationale: None,
            });
            s.add_node(GqlNode {
                id: "y".into(),
                label: "Y".into(),
                source_file: "f.rs".into(),
                source_location: None,
                community: Some(2),
                file_type: "Code".into(),
                rationale: None,
            });
        }
        let schema = Schema::build(QueryRoot, MutationRoot, EmptySubscription)
            .data(state)
            .finish();
        let res = schema.execute("{ community(id: 1) { id } }").await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let arr = data["community"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "x");
    }

    #[tokio::test]
    async fn test_query_community_empty() {
        let schema = make_schema();
        let res = schema.execute("{ community(id: 99) { id } }").await;
        assert!(res.errors.is_empty());
        assert!(res.data.into_json().unwrap()["community"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_query_shortest_path_found() {
        let schema = schema_with(&[("a", "A"), ("b", "B")], &[("a", "b", "calls")]);
        let res = schema
            .execute(r#"{ shortestPath(source: "a", target: "b") }"#)
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let path: Vec<_> = data["shortestPath"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(path, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn test_query_shortest_path_multi_hop() {
        let schema = schema_with(
            &[("a", "A"), ("b", "B"), ("c", "C")],
            &[("a", "b", "calls"), ("b", "c", "calls")],
        );
        let res = schema
            .execute(r#"{ shortestPath(source: "a", target: "c") }"#)
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let path: Vec<_> = data["shortestPath"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(path, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn test_query_shortest_path_not_found() {
        let schema = schema_with(&[("a", "A"), ("b", "B")], &[]);
        let res = schema
            .execute(r#"{ shortestPath(source: "a", target: "b") }"#)
            .await;
        assert!(res.errors.is_empty());
        assert!(res.data.into_json().unwrap()["shortestPath"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_query_shortest_path_self() {
        let schema = schema_with(&[("a", "A")], &[]);
        let res = schema
            .execute(r#"{ shortestPath(source: "a", target: "a") }"#)
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let path: Vec<_> = data["shortestPath"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(path, vec!["a"]);
    }

    #[tokio::test]
    async fn test_query_search_exact() {
        let schema = schema_with(&[("1", "AuthService"), ("2", "UserService")], &[]);
        let res = schema
            .execute(r#"{ search(query: "AuthService") { id } }"#)
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["search"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_query_search_substring() {
        let schema = schema_with(
            &[
                ("1", "AuthService"),
                ("2", "AuthController"),
                ("3", "UserService"),
            ],
            &[],
        );
        let res = schema.execute(r#"{ search(query: "auth") { id } }"#).await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["search"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_query_search_no_match() {
        let schema = schema_with(&[("1", "Foo")], &[]);
        let res = schema.execute(r#"{ search(query: "zzz") { id } }"#).await;
        assert!(res.errors.is_empty());
        assert!(res.data.into_json().unwrap()["search"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_query_stats_empty() {
        let schema = make_schema();
        let res = schema
            .execute("{ stats { nodeCount edgeCount communityCount } }")
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["stats"]["nodeCount"], 0);
        assert_eq!(data["stats"]["edgeCount"], 0);
        assert_eq!(data["stats"]["communityCount"], 0);
    }

    #[tokio::test]
    async fn test_query_stats_populated() {
        let schema = schema_with(&[("a", "A"), ("b", "B")], &[("a", "b", "calls")]);
        let res = schema.execute("{ stats { nodeCount edgeCount } }").await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["stats"]["nodeCount"], 2);
        assert_eq!(data["stats"]["edgeCount"], 1);
    }

    #[tokio::test]
    async fn test_query_god_nodes() {
        let schema = schema_with(
            &[("hub", "Hub"), ("a", "A"), ("b", "B"), ("c", "C")],
            &[
                ("hub", "a", "calls"),
                ("hub", "b", "calls"),
                ("hub", "c", "calls"),
            ],
        );
        let res = schema.execute("{ godNodes(topN: 1) { id } }").await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let arr = data["godNodes"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "hub");
    }

    #[tokio::test]
    async fn test_mutation_add_node() {
        let schema = make_schema();
        let res = schema
            .execute(r#"mutation { addNode(id: "n1", label: "Foo", sourceFile: "a.rs") { id label fileType } }"#)
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["addNode"]["id"], "n1");
        assert_eq!(data["addNode"]["label"], "Foo");
        assert_eq!(data["addNode"]["fileType"], "Code");

        let res2 = schema.execute(r#"{ node(id: "n1") { id } }"#).await;
        assert!(res2.data.into_json().unwrap()["node"]["id"] == "n1");
    }

    #[tokio::test]
    async fn test_mutation_add_edge() {
        let schema = schema_with(&[("a", "A"), ("b", "B")], &[]);
        let res = schema
            .execute(r#"mutation { addEdge(source: "a", target: "b", relation: "imports") { source target relation confidence } }"#)
            .await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        assert_eq!(data["addEdge"]["source"], "a");
        assert_eq!(data["addEdge"]["target"], "b");
        assert_eq!(data["addEdge"]["relation"], "imports");
        assert_eq!(data["addEdge"]["confidence"], "EXTRACTED");
    }

    #[tokio::test]
    async fn test_mutation_reset() {
        let schema = schema_with(&[("a", "A")], &[]);
        let res = schema.execute("mutation { reset }").await;
        assert!(res.errors.is_empty());
        assert_eq!(res.data.into_json().unwrap()["reset"], true);

        let res2 = schema.execute("{ stats { nodeCount } }").await;
        assert_eq!(res2.data.into_json().unwrap()["stats"]["nodeCount"], 0);
    }

    #[tokio::test]
    async fn test_query_edges() {
        let schema = schema_with(&[("a", "A"), ("b", "B")], &[("a", "b", "calls")]);
        let res = schema.execute("{ edges { source target relation } }").await;
        assert!(res.errors.is_empty());
        let data = res.data.into_json().unwrap();
        let arr = data["edges"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["source"], "a");
        assert_eq!(arr[0]["relation"], "calls");
    }
}
