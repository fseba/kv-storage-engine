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
                        if let Some((start, end)) = range.split_once('-') {
                            manifest.l1.push(ManifestLayerNEntry {
                                range: LayerRange {
                                    start: start.to_string(),
                                    end: end.to_string(),
                                },
                                file_name: file_name.to_string(),
                            });
                        };
                    }
                }
            }
        }
        manifest
    }

    pub fn get_latest_count(&self) -> Option<usize> {
        let parse_num = |name: &String| -> Option<usize> {
            name.strip_prefix("sst-")?
                .strip_suffix(".json")?
                .parse()
                .ok()
        };
        let max_l0 = self.l0.iter().filter_map(parse_num).max();

        let max_l1 = self.l1.iter().filter_map(|e| parse_num(&e.file_name)).max();

        // [max_l0, max_l1].into_iter().flatten().max()
        match (max_l0, max_l1) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
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
    pub range: LayerRange,
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
    L1(LayerRange),
}

#[derive(Debug)]
pub struct LayerRange {
    pub start: String,
    pub end: String,
}

impl Display for LayerRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
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
        assert_eq!(manifest.l1[0].range.start, "0");
        assert_eq!(manifest.l1[0].range.end, "100");
        assert_eq!(manifest.l1[0].file_name, "file3".to_string());
        assert_eq!(manifest.l1[1].range.start, "101");
        assert_eq!(manifest.l1[1].range.end, "200");
        assert_eq!(manifest.l1[1].file_name, "file4".to_string());
    }

    #[test]
    fn get_latest_sst_count_returns_highest_sst_file_number() {
        let manifest_content =
            "[L0]\nsst-11.json\nsst-2.json\n\n[L1]\n0-100: sst-3.json\n101-200: sst-4.json\n\n";
        let manifest = Manifest::parse(manifest_content);
        let latest_count = manifest.get_latest_count();
        assert_eq!(latest_count, Some(11))
    }

    #[test]
    fn get_latest_sst_count_returns_none_for_empty_manifest() {
        let manifest_content = "[L0]\n\n[L1]\n\n";
        let manifest = Manifest::parse(manifest_content);
        let latest_count = manifest.get_latest_count();
        assert_eq!(latest_count, None)
    }
}
