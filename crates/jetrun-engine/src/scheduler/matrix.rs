use std::collections::HashMap;

use jetrun_common::models::{MatrixConfig, StageConfig};

/// Expand a stage with a matrix config into multiple concrete stages.
/// Each combination of matrix values produces a separate stage instance.
pub fn expand_matrix(stage: &StageConfig) -> Vec<(StageConfig, HashMap<String, String>)> {
    let matrix = match &stage.matrix {
        Some(m) => m,
        None => return vec![(stage.clone(), HashMap::new())],
    };

    let combinations = generate_combinations(&matrix.values);

    combinations
        .into_iter()
        .filter(|combo| !is_excluded(combo, &matrix.exclude))
        .map(|combo| {
            let suffix = combo
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");

            let mut expanded = stage.clone();
            expanded.name = format!("{} ({})", stage.name, suffix);
            expanded.matrix = None; // Already expanded

            // Substitute matrix values in step commands
            for step in &mut expanded.steps {
                for (key, value) in &combo {
                    step.run = step.run.replace(&format!("{{{{ matrix.{key} }}}}"), value);
                    step.name = step.name.replace(&format!("{{{{ matrix.{key} }}}}"), value);
                }
            }

            (expanded, combo)
        })
        .collect()
}

fn generate_combinations(
    values: &HashMap<String, Vec<String>>,
) -> Vec<HashMap<String, String>> {
    let keys: Vec<&String> = values.keys().collect();
    let mut result = vec![HashMap::new()];

    for key in keys {
        let vals = &values[key];
        let mut new_result = Vec::new();
        for existing in &result {
            for val in vals {
                let mut combo = existing.clone();
                combo.insert(key.clone(), val.clone());
                new_result.push(combo);
            }
        }
        result = new_result;
    }

    result
}

fn is_excluded(
    combo: &HashMap<String, String>,
    excludes: &[HashMap<String, String>],
) -> bool {
    excludes.iter().any(|exclude| {
        exclude
            .iter()
            .all(|(k, v)| combo.get(k).map_or(false, |cv| cv == v))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetrun_common::models::StepConfig;

    #[test]
    fn test_expand_matrix() {
        let stage = StageConfig {
            name: "test".into(),
            depends_on: vec![],
            steps: vec![StepConfig {
                name: "Run on {{ matrix.os }}".into(),
                image: None,
                run: "echo {{ matrix.os }} {{ matrix.rust }}".into(),
                env: HashMap::new(),
                timeout_minutes: None,
                cache: None,
                artifacts: None,
            }],
            matrix: Some(MatrixConfig {
                values: HashMap::from([
                    ("os".into(), vec!["linux".into(), "macos".into()]),
                    ("rust".into(), vec!["stable".into(), "nightly".into()]),
                ]),
                exclude: vec![HashMap::from([
                    ("os".into(), "macos".into()),
                    ("rust".into(), "nightly".into()),
                ])],
            }),
            condition: None,
        };

        let expanded = expand_matrix(&stage);
        // 2x2 = 4 minus 1 excluded = 3
        assert_eq!(expanded.len(), 3);
    }

    #[test]
    fn test_no_matrix() {
        let stage = StageConfig {
            name: "build".into(),
            depends_on: vec![],
            steps: vec![StepConfig {
                name: "compile".into(),
                image: None,
                run: "cargo build".into(),
                env: HashMap::new(),
                timeout_minutes: None,
                cache: None,
                artifacts: None,
            }],
            matrix: None,
            condition: None,
        };

        let expanded = expand_matrix(&stage);
        assert_eq!(expanded.len(), 1);
    }
}
