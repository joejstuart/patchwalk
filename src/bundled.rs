use crate::model::Result;
use std::path::PathBuf;

/// A private disposable demo, independent of the build machine's source tree.
pub struct Demo(pub PathBuf);

impl Demo {
    pub fn create() -> Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("patchwalk-demo-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root)?;
        let demo = Self(root);
        for (path, content) in [
            ("proposal.json", include_str!("../demo/proposal.json")),
            (
                "src/routes/orders.ts",
                include_str!("../demo/src/routes/orders.ts"),
            ),
            (
                "src/orders/service.ts",
                include_str!("../demo/src/orders/service.ts"),
            ),
            (
                "src/orders/validation.ts",
                include_str!("../demo/src/orders/validation.ts"),
            ),
            (
                "src/returns/service.ts",
                include_str!("../demo/src/returns/service.ts"),
            ),
            (
                "tests/orders.test.ts",
                include_str!("../demo/tests/orders.test.ts"),
            ),
        ] {
            let path = demo.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, content)?;
        }
        Ok(demo)
    }
}

impl Drop for Demo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn agent_guide(mode: &str) -> Result<()> {
    let (prompt, schema) = match mode {
        "planned" => (
            include_str!("../prompts/generate-proposal.txt"),
            include_str!("../schemas/proposal-v1.schema.json"),
        ),
        "implemented" => (
            include_str!("../prompts/record-implemented-change.txt"),
            include_str!("../schemas/proposal-v2.schema.json"),
        ),
        _ => return Err("--agent-guide expects planned or implemented".into()),
    };
    println!(
        "{prompt}\nThe schema is embedded below; no source checkout is required.\n\n--- JSON schema ---\n{schema}"
    );
    Ok(())
}
