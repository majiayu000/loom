use std::path::Path;

pub(crate) fn all_paths(root: &Path) -> Vec<String> {
    fn visit(base: &Path, path: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("path entry").path();
            out.push(
                path.strip_prefix(base)
                    .expect("relative path")
                    .display()
                    .to_string(),
            );
            if path.is_dir() {
                visit(base, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out.sort();
    out
}
