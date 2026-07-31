use std::fmt::Display;

#[derive(Debug, Default)]
pub struct Manifest {
    pub l0: Vec<String>,
    pub l1: Vec<ManifestLayerNEntry>,
}

impl Manifest {
    pub fn parse(content: &str) -> Result<Self, ParseError> {
        let mut manifest = Manifest::default();
        let mut lines = content.lines();

        while let Some(line) = lines.next() {
            match line {
                "[L0]" => {
                    for l0_line in lines
                        .by_ref()
                        .take_while(|l| !l.is_empty() && !l.starts_with('['))
                    {
                        manifest.l0.push(l0_line.to_string());
                    }
                }
                "[L1]" => {
                    for l1_line in lines
                        .by_ref()
                        .take_while(|l| !l.is_empty() && !l.starts_with('['))
                    {
                        let (range, file_name) = l1_line
                            .split_once(": ")
                            .ok_or_else(|| ParseError::MalformedLine(l1_line.to_string()))?;
                        let (start, end) = range
                            .split_once('-')
                            .ok_or_else(|| ParseError::MalformedLine(l1_line.to_string()))?;
                        manifest.l1.push(ManifestLayerNEntry {
                            range: LayerRange {
                                start: start.to_string(),
                                end: end.to_string(),
                            },
                            file_name: file_name.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(manifest)
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

    pub fn get_l1_files_within_range(&self, start: &str, end: &str) -> Vec<String> {
        self.l1
            .iter()
            .filter(|f| f.range.start.as_str() < end && f.range.end.as_str() >= start)
            .map(|f| f.file_name.clone())
            .collect()
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

#[derive(Debug, Default)]
pub struct ManifestLayerNEntry {
    pub range: LayerRange,
    pub file_name: String,
}

impl Display for ManifestLayerNEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.range, self.file_name)
    }
}

#[derive(Debug, Default)]
pub enum ManifestLayer {
    #[default]
    L0,
    L1(LayerRange),
}

#[derive(Debug, Default)]
pub struct LayerRange {
    pub start: String,
    pub end: String,
}

impl Display for LayerRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

#[derive(Debug)]
pub enum ParseError {
    MalformedLine(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MalformedLine(line) => write!(f, "malformed manifest line: {line}"),
        }
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod test {
    use super::Manifest;

    #[test]
    fn parse_manifest_creates_correct_struct() {
        let manifest_content = "[L0]\nfile1\nfile2\n\n[L1]\n0-100: file3\n101-200: file4\n\n";
        let manifest = Manifest::parse(manifest_content).unwrap();
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
        let manifest = Manifest::parse(manifest_content).unwrap();
        let latest_count = manifest.get_latest_count();
        assert_eq!(latest_count, Some(11))
    }

    #[test]
    fn get_latest_sst_count_returns_none_for_empty_manifest() {
        let manifest_content = "[L0]\n\n[L1]\n\n";
        let manifest = Manifest::parse(manifest_content).unwrap();
        let latest_count = manifest.get_latest_count();
        assert_eq!(latest_count, None)
    }

    #[test]
    fn get_files_with_range_return_file_names() {
        let manifest_content = "[L0]\nsst-11.json\nsst-2.json\n\n[L1]\na-c: sst-3.json\nc-dd: sst-4.json\ndd-zz: sst-5.json\n\n";
        let manifest = Manifest::parse(manifest_content).unwrap();
        let start = "a";
        let end = "dd";
        let files = manifest.get_l1_files_within_range(start, end);
        assert!(files.contains(&"sst-3.json".to_string()));
        assert!(files.contains(&"sst-4.json".to_string()));
        assert!(!files.contains(&"sst-5.json".to_string()));
        assert!(!files.contains(&"sst-2.json".to_string()));
        assert!(!files.contains(&"sst-11.json".to_string()));
    }
}
