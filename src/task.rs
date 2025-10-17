pub fn normalize_task_query(s: &str) -> String {
    s.replace(':', ".").trim().to_string()
}

pub fn display_task_name(s: &str) -> String {
    s.replace('.', ":")
}

/// Resolve a child task reference relative to a parent namespace.
/// If `child` contains a dot or colon, it is treated as absolute.
/// Otherwise, it is appended to the parent's name with a dot.
pub fn resolve_child_name(parent: &str, child: &str) -> String {
    let child_norm = normalize_task_query(child);
    if child_norm.contains('.') {
        child_norm
    } else if parent.is_empty() {
        child_norm
    } else {
        format!("{}.{}", parent, child_norm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_colon_to_dot() {
        assert_eq!(normalize_task_query("frontend:build"), "frontend.build");
        assert_eq!(normalize_task_query("api:migrate"), "api.migrate");
    }

    #[test]
    fn displays_dot_as_colon() {
        assert_eq!(display_task_name("frontend.build"), "frontend:build");
        assert_eq!(display_task_name("a.b.c"), "a:b:c");
    }

    #[test]
    fn round_trip() {
        let original = "frontend.build.assets";
        let shown = display_task_name(original);
        let back = normalize_task_query(&shown);
        assert_eq!(back, original);
    }

    #[test]
    fn resolves_child_names() {
        assert_eq!(resolve_child_name("build", "frontend"), "build.frontend");
        assert_eq!(
            resolve_child_name("build", "frontend.build"),
            "frontend.build"
        );
        assert_eq!(resolve_child_name("", "api.migrate"), "api.migrate");
        assert_eq!(resolve_child_name("group.sub", "task"), "group.sub.task");
        assert_eq!(resolve_child_name("group.sub", "api:deploy"), "api.deploy");
    }
}
