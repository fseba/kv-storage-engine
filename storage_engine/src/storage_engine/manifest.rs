use std::fmt::Display;

#[derive(Debug)]
pub struct Manifest {
    pub l0: Vec<String>,
    pub l1: Vec<ManifestLayerNEntry>,
}

impl Manifest {
    pub fn parse(content: &str) -> Self {
        let mut lines = content.lines();
        let mut manifest = Manifest {
            l0: Vec::new(),
            l1: Vec::new(),
        };

        while let Some(line) = lines.next() {
            if line == "[L0]" {
                for l0_line in lines.by_ref() {
                    if l0_line.is_empty() {
                        break;
                    }
                    manifest.l0.push(l0_line.to_string());
                }
            } else if line == "[L1]" {
                for l1_line in lines.by_ref() {
                    if l1_line.is_empty() {
                        break;
                    }
                    if let Some((range, file_name)) = l1_line.split_once(": ") {
                        manifest.l1.push(ManifestLayerNEntry {
                            range: range.to_string(),
                            file_name: file_name.to_string(),
                        });
                    }
                }
            }
        }
        manifest
    }
}

impl Display for Manifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "[L0]")?;
        for l0_entry in &self.l0 {
            writeln!(f, "{}", l0_entry)?;
        }
        writeln!(f)?;

        writeln!(f, "[L1]")?;
        for l1_entry in &self.l1 {
            writeln!(f, "{}", l1_entry)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ManifestLayerNEntry {
    pub range: String,
    pub file_name: String,
}

impl Display for ManifestLayerNEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}: {}", self.range, self.file_name)
    }
}

#[derive(Debug)]
pub enum ManifestLayer {
    L0,
    L1(String),
}

#[cfg(test)]
mod test {
    use super::Manifest;

    #[test]
    fn parse_manifest_creates_correct_struct() {
        let manifest_content = "[L0]\nfile1\nfile2\n\n[L1]\n0-100: file3\n101-200: file4\n\n";
        let manifest = Manifest::parse(manifest_content);
        assert_eq!(manifest.l0, vec!["file1".to_string(), "file2".to_string()]);
        assert_eq!(manifest.l1.len(), 2);
        assert_eq!(manifest.l1[0].range, "0-100".to_string());
        assert_eq!(manifest.l1[0].file_name, "file3".to_string());
        assert_eq!(manifest.l1[1].range, "101-200".to_string());
        assert_eq!(manifest.l1[1].file_name, "file4".to_string());
    }
}
