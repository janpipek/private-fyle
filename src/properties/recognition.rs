//! Ranked filename and bounded content recognition clues for a local file.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::properties::{
    GroupId, GroupStatus, Probe, PropertyField, PropertyGroup, PropertyProvider, PropertyValue,
};

/// Exposes ranked MIME candidates and their evidence for a local regular file.
pub struct Provider;

impl PropertyProvider for Provider {
    fn id(&self) -> &'static str {
        "recognition"
    }
    fn matches(&self, probe: &Probe) -> bool {
        probe.is_file
    }

    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let content = crate::recognition::read_head(&probe.path);
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let candidates = crate::recognition::detect(
            &probe.name,
            false,
            content.as_deref().ok(),
            None,
            &crate::recognition::NativeDetector,
        );
        let mut fields = vec![PropertyField {
            id: "scope".into(),
            value: PropertyValue::Text(
                "Filename and first 8 KiB clues; not format validation".into(),
            ),
        }];
        fields.extend(candidates.iter().enumerate().flat_map(|(rank, candidate)| {
            [
                PropertyField {
                    id: format!("candidate_{}_mime", rank + 1),
                    value: PropertyValue::Text(candidate.mime.clone()),
                },
                PropertyField {
                    id: format!("candidate_{}_evidence", rank + 1),
                    value: PropertyValue::Text(evidence_label(candidate.evidence).into()),
                },
            ]
        }));
        if let Err(error) = content {
            fields.push(PropertyField {
                id: "content_read".into(),
                value: PropertyValue::Text(format!("Content read failed: {error}")),
            });
        }
        Ok(vec![PropertyGroup {
            id: GroupId("recognition".into()),
            status: GroupStatus::Available,
            fields,
            groups: Vec::new(),
            tables: Vec::new(),
        }])
    }
}

fn evidence_label(evidence: crate::recognition::Evidence) -> &'static str {
    match evidence {
        crate::recognition::Evidence::Signature => "Content signature",
        crate::recognition::Evidence::Name => "Filename",
        crate::recognition::Evidence::Heuristic => "Content heuristic",
        crate::recognition::Evidence::Fallback => "Fallback",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use crate::properties::{Probe, PropertyProvider, PropertyValue};

    #[test]
    fn exposes_ranked_signature_and_filename_clues() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("misleading.txt");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
        let probe = Probe {
            path,
            name: "misleading.txt".into(),
            ext: "txt".into(),
            is_dir: false,
            is_file: true,
            size: 8,
        };
        let group = super::Provider
            .read(&probe, &Arc::new(AtomicBool::new(false)))
            .unwrap()
            .remove(0);
        let values: Vec<_> = group
            .fields
            .iter()
            .map(|field| match &field.value {
                PropertyValue::Text(value) => value.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert!(values.contains(&"image/png"));
        assert!(values.contains(&"Content signature"));
        assert!(values.contains(&"text/plain"));
    }

    #[test]
    fn failed_reads_keep_name_clue_and_report_content_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("notes.txt");
        let probe = Probe {
            path,
            name: "notes.txt".into(),
            ext: "txt".into(),
            is_dir: false,
            is_file: true,
            size: 0,
        };
        let group = super::Provider
            .read(&probe, &Arc::new(AtomicBool::new(false)))
            .unwrap()
            .remove(0);
        assert!(
            group
                .fields
                .iter()
                .any(|field| field.id == "candidate_1_mime"
                    && field.value == PropertyValue::Text("text/plain".into()))
        );
        assert!(group.fields.iter().any(|field| field.id == "content_read"));
        assert!(
            !group
                .fields
                .iter()
                .any(|field| field.value == PropertyValue::Text("Content heuristic".into()))
        );
    }

    #[test]
    fn skips_directories_and_observes_cancellation() {
        let dir = tempfile::TempDir::new().unwrap();
        let probe = Probe {
            path: dir.path().into(),
            name: "folder".into(),
            ext: String::new(),
            is_dir: true,
            is_file: false,
            size: 0,
        };
        assert!(!super::Provider.matches(&probe));
        let cancel = Arc::new(AtomicBool::new(true));
        assert_eq!(
            super::Provider.read(&probe, &cancel).unwrap_err().kind(),
            std::io::ErrorKind::Interrupted
        );
    }
}
