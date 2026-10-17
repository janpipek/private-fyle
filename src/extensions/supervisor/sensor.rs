//! Real-helper integration checks for the documented sensor package example.

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use crate::data_providers::{Completeness, ContributionId, ResultState, Unit, Value};
    use crate::extensions::loading::Package;
    use crate::extensions::manifest::{self, PackageId};
    use crate::extensions::results::{PreparedResult, PropertiesResult};
    use crate::extensions::supervisor::{ContributionWork, Supervisor};
    use crate::location::Location;
    use crate::preview_scene::NodeKind;
    use tempfile::TempDir;

    const MANIFEST: &str = include_str!("../../../examples/extensions/sensor/manifest.toml");
    const SCRIPT: &str = include_str!("../../../examples/extensions/sensor/main.lua");
    const COMPLETE: &str =
        include_str!("../../../examples/extensions/sensor/fixtures/complete.sens");
    const TRUNCATED: &str =
        include_str!("../../../examples/extensions/sensor/fixtures/truncated.sens");
    const MALFORMED: &str =
        include_str!("../../../examples/extensions/sensor/fixtures/malformed.sens");

    const SUMMARY: &str = "extension.sensor.reader.summary";
    const COLUMN: &str = "extension.sensor.reader.distance-cell";
    const VIEWER: &str = "extension.sensor.reader.graph";

    fn package() -> Arc<Package> {
        let id = PackageId::new("sensor").expect("fixture package ID is valid");
        let manifest = manifest::parse(&id, MANIFEST).expect("sensor example manifest is valid");
        Arc::new(Package {
            manifest,
            source: SCRIPT.into(),
        })
    }

    fn supervisor() -> Supervisor {
        let launcher = Arc::new(|| {
            let mut command = Command::new(std::env::current_exe()?);
            command
                .args([
                    "--exact",
                    "extensions::supervisor::process::tests::worker_subprocess_fixture",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("FYLE_EXTENSION_WORKER_FIXTURE", "worker");
            Ok(command)
        });
        Supervisor::with_launcher(launcher, true).expect("test supervisor starts")
    }

    fn run(
        supervisor: &Supervisor,
        package: &Arc<Package>,
        root: &std::path::Path,
        source_name: &str,
        contribution: &str,
    ) -> PreparedResult {
        let source = root.join(source_name);
        let ticket = supervisor
            .submit_contribution(ContributionWork {
                package: package.clone(),
                contribution: ContributionId::new(contribution)
                    .expect("fixture contribution ID is valid"),
                source: Location::local(source).expect("fixture source path is absolute"),
                name: source_name.into(),
                is_dir: false,
                generation_cancelled: Arc::new(AtomicBool::new(false)),
            })
            .expect("sensor contribution request is admitted");
        ticket.wait().expect("sensor helper returns a result")
    }

    fn write_fixture(root: &std::path::Path, name: &str, contents: &str) {
        std::fs::write(root.join(name), contents).expect("write sensor input fixture");
    }

    fn ready_quantity(result: &ResultState<Value>, unit: Unit) -> (f64, &Completeness) {
        let ResultState::Ready {
            value: Value::Quantity(quantity),
            completeness,
        } = result
        else {
            panic!("expected a ready quantity result, got {result:?}");
        };
        assert_eq!(quantity.unit(), unit);
        (quantity.value(), completeness)
    }

    fn property_value(result: &PropertiesResult, index: usize, unit: Unit) -> (f64, Completeness) {
        let PropertiesResult::Ready(fields) = result else {
            panic!("expected ready Properties fields, got {result:?}");
        };
        let (value, completeness) = ready_quantity(&fields[index].state, unit);
        (value, completeness.clone())
    }

    fn scene(result: PreparedResult) -> (crate::preview_scene::Scene, Completeness) {
        let PreparedResult::Viewer(ResultState::Ready {
            value,
            completeness,
        }) = result
        else {
            panic!("expected a ready viewer scene, got {result:?}");
        };
        (value, completeness)
    }

    #[test]
    fn complete_sensor_example_returns_summary_table_and_four_point_scene() {
        let temp = TempDir::new().expect("temporary sensor fixture directory");
        write_fixture(temp.path(), "complete.sens", COMPLETE);
        let package = package();
        let supervisor = supervisor();

        let summary = run(&supervisor, &package, temp.path(), "complete.sens", SUMMARY);
        let PreparedResult::Properties(summary) = summary else {
            panic!("expected summary Properties result, got {summary:?}");
        };
        assert_eq!(
            property_value(&summary, 0, Unit::Seconds),
            (30.0, Completeness::Complete)
        );
        assert_eq!(
            property_value(&summary, 1, Unit::Metres),
            (95.0, Completeness::Complete)
        );

        let column = run(&supervisor, &package, temp.path(), "complete.sens", COLUMN);
        let PreparedResult::Column(column) = column else {
            panic!("expected distance column result, got {column:?}");
        };
        assert_eq!(ready_quantity(&column, Unit::Metres).0, 95.0);
        assert_eq!(
            ready_quantity(&column, Unit::Metres).1,
            &Completeness::Complete
        );

        let (scene, completeness) = scene(run(
            &supervisor,
            &package,
            temp.path(),
            "complete.sens",
            VIEWER,
        ));
        assert_eq!(completeness, Completeness::Complete);
        let NodeKind::Stack { children, .. } = scene.root().kind() else {
            panic!("expected a stack root");
        };
        let NodeKind::Canvas { paths, .. } = children[2].kind() else {
            panic!("expected a line canvas");
        };
        assert_eq!(paths[0].points().len(), 4);
        let NodeKind::Table { columns, rows } = children[4].kind() else {
            panic!("expected typed sample table");
        };
        assert_eq!(columns.len(), 2);
        assert_eq!(rows.len(), 4);
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    #[test]
    fn truncated_sensor_example_preserves_lower_bounds_and_partial_scene() {
        let temp = TempDir::new().expect("temporary sensor fixture directory");
        write_fixture(temp.path(), "truncated.sens", TRUNCATED);
        let package = package();
        let supervisor = supervisor();

        let summary = run(
            &supervisor,
            &package,
            temp.path(),
            "truncated.sens",
            SUMMARY,
        );
        let PreparedResult::Properties(summary) = summary else {
            panic!("expected summary Properties result, got {summary:?}");
        };
        assert_eq!(
            property_value(&summary, 0, Unit::Seconds),
            (127.0, Completeness::LowerBound)
        );
        assert_eq!(
            property_value(&summary, 1, Unit::Metres),
            (381.0, Completeness::LowerBound)
        );

        let column = run(&supervisor, &package, temp.path(), "truncated.sens", COLUMN);
        let PreparedResult::Column(column) = column else {
            panic!("expected distance column result, got {column:?}");
        };
        assert_eq!(ready_quantity(&column, Unit::Metres).0, 381.0);
        assert_eq!(
            ready_quantity(&column, Unit::Metres).1,
            &Completeness::LowerBound
        );

        let (scene, completeness) = scene(run(
            &supervisor,
            &package,
            temp.path(),
            "truncated.sens",
            VIEWER,
        ));
        assert!(matches!(completeness, Completeness::Truncated(_)));
        let NodeKind::Stack { children, .. } = scene.root().kind() else {
            panic!("expected a stack root");
        };
        assert!(
            matches!(children[3].kind(), NodeKind::Text { text, .. } if text.contains("partial input"))
        );
        supervisor.shutdown().expect("test supervisor shuts down");
    }

    #[test]
    fn nonmonotonic_sensor_samples_fail_each_contribution() {
        let temp = TempDir::new().expect("temporary sensor fixture directory");
        write_fixture(temp.path(), "malformed.sens", MALFORMED);
        let package = package();
        let supervisor = supervisor();

        for contribution in [SUMMARY, COLUMN, VIEWER] {
            let result = run(
                &supervisor,
                &package,
                temp.path(),
                "malformed.sens",
                contribution,
            );
            match result {
                PreparedResult::Properties(PropertiesResult::Failed(reason))
                    if contribution == SUMMARY =>
                {
                    assert!(reason.contains("sample times must increase"))
                }
                PreparedResult::Column(ResultState::Failed(reason)) if contribution == COLUMN => {
                    assert!(reason.contains("sample times must increase"));
                }
                PreparedResult::Viewer(ResultState::Failed(reason)) if contribution == VIEWER => {
                    assert!(reason.contains("sample times must increase"));
                }
                other => panic!("expected nonmonotonic parse failure, got {other:?}"),
            }
        }
        supervisor.shutdown().expect("test supervisor shuts down");
    }
}
