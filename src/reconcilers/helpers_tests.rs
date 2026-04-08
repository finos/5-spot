#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use crate::constants::{
        ALLOWED_BOOTSTRAP_API_GROUPS, ALLOWED_INFRASTRUCTURE_API_GROUPS, MAX_DURATION_SECS,
    };
    use std::collections::BTreeMap;

    // ========================================================================
    // parse_duration — overflow & bounds protection
    // ========================================================================

    #[test]
    fn test_parse_duration_seconds() {
        let d = parse_duration("30s").unwrap();
        assert_eq!(d.as_secs(), 30);
    }

    #[test]
    fn test_parse_duration_minutes() {
        let d = parse_duration("5m").unwrap();
        assert_eq!(d.as_secs(), 300);
    }

    #[test]
    fn test_parse_duration_hours() {
        let d = parse_duration("1h").unwrap();
        assert_eq!(d.as_secs(), 3600);
    }

    #[test]
    fn test_parse_duration_max_allowed() {
        // 24h == MAX_DURATION_SECS exactly — should be accepted
        let d = parse_duration("24h").unwrap();
        assert_eq!(d.as_secs(), MAX_DURATION_SECS);
    }

    #[test]
    fn test_parse_duration_exceeds_max_hours() {
        let err = parse_duration("25h").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exceeds maximum"),
            "expected 'exceeds maximum' in error, got: {msg}"
        );
    }

    #[test]
    fn test_parse_duration_exceeds_max_seconds() {
        // 86401s is one second over the 24h limit
        let err = parse_duration("86401s").unwrap_err();
        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_parse_duration_overflow_u64() {
        // This would overflow u64 on multiplication: 9999999999999999h * 3600
        let err = parse_duration("9999999999999999h").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("overflow") || msg.contains("exceeds maximum"),
            "expected overflow or bounds error, got: {msg}"
        );
    }

    #[test]
    fn test_parse_duration_overflow_minutes() {
        // u64::MAX / 60 + 1 should overflow
        let value = u64::MAX / 60 + 1;
        let input = format!("{value}m");
        let err = parse_duration(&input).unwrap_err();
        assert!(
            err.to_string().contains("overflow") || err.to_string().contains("exceeds maximum")
        );
    }

    #[test]
    fn test_parse_duration_empty() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("   ").is_err());
    }

    #[test]
    fn test_parse_duration_invalid_unit() {
        let err = parse_duration("5d").unwrap_err();
        assert!(err.to_string().contains("Invalid duration unit"));
    }

    #[test]
    fn test_parse_duration_invalid_value() {
        assert!(parse_duration("abch").is_err());
        assert!(parse_duration("s").is_err());
    }

    // ========================================================================
    // validate_labels — reserved prefix rejection
    // ========================================================================

    #[test]
    fn test_validate_labels_clean() {
        let mut labels = BTreeMap::new();
        labels.insert("app".to_string(), "my-app".to_string());
        labels.insert("team".to_string(), "platform".to_string());
        assert!(validate_labels(&labels, "labels").is_ok());
    }

    #[test]
    fn test_validate_labels_rejects_kubernetes_io() {
        let mut labels = BTreeMap::new();
        labels.insert("kubernetes.io/hostname".to_string(), "node1".to_string());
        let err = validate_labels(&labels, "labels").unwrap_err();
        assert!(err.to_string().contains("reserved prefix"));
        assert!(err.to_string().contains("kubernetes.io/"));
    }

    #[test]
    fn test_validate_labels_rejects_k8s_io() {
        let mut labels = BTreeMap::new();
        labels.insert("k8s.io/something".to_string(), "val".to_string());
        assert!(validate_labels(&labels, "labels").is_err());
    }

    #[test]
    fn test_validate_labels_rejects_cluster_x_k8s_io() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "cluster.x-k8s.io/cluster-name".to_string(),
            "injected-cluster".to_string(),
        );
        let err = validate_labels(&labels, "labels").unwrap_err();
        assert!(err.to_string().contains("reserved prefix"));
    }

    #[test]
    fn test_validate_labels_rejects_5spot_io() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "5spot.io/scheduled-machine".to_string(),
            "injected".to_string(),
        );
        assert!(validate_labels(&labels, "labels").is_err());
    }

    #[test]
    fn test_validate_labels_empty_map() {
        let labels = BTreeMap::new();
        assert!(validate_labels(&labels, "labels").is_ok());
    }

    #[test]
    fn test_validate_annotations_rejects_reserved() {
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "kubernetes.io/created-by".to_string(),
            "attacker".to_string(),
        );
        let err = validate_labels(&annotations, "annotations").unwrap_err();
        assert!(err.to_string().contains("annotations"));
        assert!(err.to_string().contains("reserved prefix"));
    }

    // ========================================================================
    // validate_api_group — allowlist enforcement
    // ========================================================================

    #[test]
    fn test_validate_api_group_valid_bootstrap() {
        assert!(validate_api_group(
            "bootstrap.cluster.x-k8s.io/v1beta1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_api_group_valid_k0smotron_bootstrap() {
        assert!(validate_api_group(
            "k0smotron.io/v1beta1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_api_group_valid_infrastructure() {
        assert!(validate_api_group(
            "infrastructure.cluster.x-k8s.io/v1beta1",
            ALLOWED_INFRASTRUCTURE_API_GROUPS,
            "infrastructure"
        )
        .is_ok());
    }

    #[test]
    fn test_validate_api_group_rejects_core_api() {
        // Core API (no slash) must be rejected
        let err = validate_api_group("v1", ALLOWED_BOOTSTRAP_API_GROUPS, "bootstrap").unwrap_err();
        assert!(err.to_string().contains("namespaced API group"));
    }

    #[test]
    fn test_validate_api_group_rejects_rbac() {
        let err = validate_api_group(
            "rbac.authorization.k8s.io/v1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
        assert!(err.to_string().contains("rbac.authorization.k8s.io"));
    }

    #[test]
    fn test_validate_api_group_rejects_apps() {
        let err = validate_api_group(
            "apps/v1",
            ALLOWED_INFRASTRUCTURE_API_GROUPS,
            "infrastructure",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn test_validate_api_group_rejects_wrong_side() {
        // Infrastructure group used as bootstrap should be rejected
        let err = validate_api_group(
            "infrastructure.cluster.x-k8s.io/v1beta1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn test_validate_api_group_rejects_kube_system_trick() {
        // Attempt to sneak in a system API group
        let err = validate_api_group(
            "admissionregistration.k8s.io/v1",
            ALLOWED_BOOTSTRAP_API_GROUPS,
            "bootstrap",
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }
}
