pub fn normalize_task_query(s: &str) -> String {
    s.replace(':', ".").trim().to_string()
}

pub fn display_task_name(s: &str) -> String {
    s.replace('.', ":")
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
}

