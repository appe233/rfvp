use super::*;

#[test]
fn session_composition_allows_combined_counts_above_one_plan_and_bounds_total_bytes() {
    let mut merger = Merger::default();
    let mut diagnostics = Vec::new();
    for start in [4, 60_004] {
        let plan = Plan {
            strings: (start..start + 60_000)
                .map(|offset| abi::Replacement {
                    offset,
                    text: String::new(),
                })
                .collect(),
            resources: Vec::new(),
        };
        merger
            .stage(
                "test",
                plan,
                8 * 1024 * 1024,
                &BTreeMap::new(),
                &mut diagnostics,
            )
            .unwrap();
    }
    assert_eq!(merger.finish(4..120_004).unwrap().0.len(), 120_000);
    let mut merger = Merger::default();
    for _ in 0..4 {
        merger
            .stage(
                "test",
                Plan::default(),
                abi::MAX_PLAN,
                &BTreeMap::new(),
                &mut diagnostics,
            )
            .unwrap();
    }
    assert!(merger
        .stage(
            "test",
            Plan::default(),
            1,
            &BTreeMap::new(),
            &mut diagnostics
        )
        .is_err());
}

#[test]
fn session_conflict_diagnostics_are_bounded_without_losing_later_plan() {
    let mut merger = Merger::default();
    let mut diagnostics = Vec::new();
    let plan = || Plan {
        strings: (4..40_004)
            .map(|offset| abi::Replacement {
                offset,
                text: "replacement".into(),
            })
            .collect(),
        resources: Vec::new(),
    };
    merger
        .stage(
            "first",
            plan(),
            4_000_000,
            &BTreeMap::new(),
            &mut diagnostics,
        )
        .unwrap();
    merger
        .stage(
            "second",
            plan(),
            4_000_000,
            &BTreeMap::new(),
            &mut diagnostics,
        )
        .unwrap();
    assert!(merger.suppressed > 0);
    assert!(diagnostics.iter().map(String::len).sum::<usize>() <= 1024 * 1024);
    assert_eq!(
        merger.finish(4..40_004).unwrap().0.get(40_003),
        Some("replacement")
    );
}

#[cfg(feature = "sakura-moyu")]
#[test]
fn bundled_alias_warns_only_for_unverified_scripts() {
    let manifest = bundled_manifest();
    let verified = rfvp_plugin_sakura::VERIFIED_SCRIPT_SHA256[0];
    // Verified identities stay valid manifest data and suppress only the warning.
    manifest.validate(verified, false).unwrap();
    assert_eq!(bundled_identity_diagnostic(&manifest, verified), None);
    let other = "0".repeat(64);
    manifest.validate(&other, true).unwrap();
    assert!(manifest.validate(&other, false).is_err());
    assert!(bundled_identity_diagnostic(&manifest, &other)
        .unwrap()
        .contains("unverified script identity"));
}
