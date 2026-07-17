use std::fs;

use serde_json::json;

use super::*;

#[test]
fn symlink_copy_materialize() {
    let mut methods = vec!["copy", "materialize"];
    if cfg!(unix) {
        methods.push("symlink");
    }

    for method in methods {
        let fixture = projected_fixture_with_method(method);
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "{method} plan failed: {plan}");
        assert_eq!(plan["data"]["effects"][0]["method"], json!(method));
        assert_eq!(plan["data"]["effects"][0]["effect"], json!("refresh"));

        let projection = fixture.target.path().join("demo");
        match method {
            "symlink" => assert!(
                fs::symlink_metadata(&projection)
                    .expect("symlink projection")
                    .file_type()
                    .is_symlink()
            ),
            "copy" | "materialize" => {
                assert!(projection.is_dir(), "{method} projection must be a tree");
                assert!(projection.join("SKILL.md").is_file());
            }
            unexpected => panic!("unexpected projection method {unexpected}"),
        }
    }
}
