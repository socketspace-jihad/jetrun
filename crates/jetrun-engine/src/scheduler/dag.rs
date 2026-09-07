use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use jetrun_common::error::JetrunError;
use jetrun_common::models::StageConfig;

/// Schedules stages based on their dependency graph (DAG).
/// Computes topological sort and execution levels once in `new()` — cached for reuse.
pub struct DagScheduler {
    graph: DiGraph<String, ()>,
    node_map: HashMap<String, NodeIndex>,
    /// Cached execution levels: stages in the same level run in parallel.
    cached_levels: Vec<Vec<String>>,
}

impl DagScheduler {
    /// Build a DAG from stage configurations.
    /// Validates no cycles exist and pre-computes execution levels.
    pub fn new(stages: &[StageConfig]) -> Result<Self, JetrunError> {
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::new();

        // Add all stages as nodes
        for stage in stages {
            let idx = graph.add_node(stage.name.clone());
            node_map.insert(stage.name.clone(), idx);
        }

        // Add dependency edges
        for stage in stages {
            let stage_idx = node_map[&stage.name];
            for dep in &stage.depends_on {
                let dep_idx = node_map.get(dep).ok_or_else(|| {
                    JetrunError::InvalidConfig(format!(
                        "Stage '{}' depends on unknown stage '{}'",
                        stage.name, dep
                    ))
                })?;
                graph.add_edge(*dep_idx, stage_idx, ());
            }
        }

        // Single topological sort — validates no cycles and produces ordering
        let topo_order = toposort(&graph, None).map_err(|_| {
            JetrunError::InvalidConfig("Circular dependency detected in pipeline stages".into())
        })?;

        // Pre-compute execution levels from the cached topo order
        let cached_levels = Self::compute_levels(&graph, &topo_order);

        Ok(Self {
            graph,
            node_map,
            cached_levels,
        })
    }

    /// Returns stages grouped by execution level.
    /// Stages in the same level can run in parallel.
    /// This is O(1) — returns the pre-computed cache.
    pub fn execution_levels(&self) -> &[Vec<String>] {
        &self.cached_levels
    }

    /// Returns the total number of stages.
    pub fn stage_count(&self) -> usize {
        self.node_map.len()
    }

    /// Returns the number of parallel execution levels.
    pub fn level_count(&self) -> usize {
        self.cached_levels.len()
    }

    fn compute_levels(graph: &DiGraph<String, ()>, sorted: &[NodeIndex]) -> Vec<Vec<String>> {
        let mut levels: Vec<Vec<String>> = Vec::new();
        let mut node_levels: HashMap<NodeIndex, usize> = HashMap::new();

        for &node in sorted {
            let level = graph
                .neighbors_directed(node, petgraph::Direction::Incoming)
                .map(|parent| node_levels[&parent] + 1)
                .max()
                .unwrap_or(0);

            node_levels.insert(node, level);

            while levels.len() <= level {
                levels.push(Vec::new());
            }
            levels[level].push(graph[node].clone());
        }

        levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetrun_common::models::{StageConfig, StepConfig};
    use std::collections::HashMap;

    fn make_stage(name: &str, depends_on: Vec<&str>) -> StageConfig {
        StageConfig {
            name: name.into(),
            depends_on: depends_on.into_iter().map(String::from).collect(),
            steps: vec![StepConfig {
                name: "step".into(),
                image: None,
                run: "echo hello".into(),
                env: HashMap::new(),
                timeout_minutes: None,
                cache: None,
                artifacts: None,
            }],
            matrix: None,
            condition: None,
        }
    }

    #[test]
    fn test_linear_dag() {
        let stages = vec![
            make_stage("lint", vec![]),
            make_stage("test", vec!["lint"]),
            make_stage("build", vec!["test"]),
        ];
        let scheduler = DagScheduler::new(&stages).unwrap();
        let levels = scheduler.execution_levels();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], vec!["lint"]);
        assert_eq!(levels[1], vec!["test"]);
        assert_eq!(levels[2], vec!["build"]);
    }

    #[test]
    fn test_parallel_dag() {
        let stages = vec![
            make_stage("lint", vec![]),
            make_stage("test", vec![]),
            make_stage("deploy", vec!["lint", "test"]),
        ];
        let scheduler = DagScheduler::new(&stages).unwrap();
        let levels = scheduler.execution_levels();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].len(), 2); // lint and test in parallel
        assert_eq!(levels[1], vec!["deploy"]);
    }

    #[test]
    fn test_cycle_detection() {
        let stages = vec![
            make_stage("a", vec!["b"]),
            make_stage("b", vec!["a"]),
        ];
        assert!(DagScheduler::new(&stages).is_err());
    }

    #[test]
    fn test_execution_levels_is_cached() {
        let stages = vec![
            make_stage("a", vec![]),
            make_stage("b", vec!["a"]),
        ];
        let scheduler = DagScheduler::new(&stages).unwrap();
        // Calling execution_levels multiple times should return the same reference
        let l1 = scheduler.execution_levels();
        let l2 = scheduler.execution_levels();
        assert_eq!(l1.len(), l2.len());
        assert_eq!(scheduler.level_count(), 2);
        assert_eq!(scheduler.stage_count(), 2);
    }
}
