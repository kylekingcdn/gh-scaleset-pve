use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::fs;

pub(crate) struct CloudConfigGenerator {
    template_path: String,
    output_path: String,
}
impl CloudConfigGenerator {
    pub fn new(template_path: String, output_path: String) -> Self {
        Self {
            template_path,
            output_path,
        }
    }
    fn read_template(&self) -> color_eyre::Result<String> {
        Ok(fs::read_to_string(&self.template_path)?)
    }
    fn write_output(&self, content: String) -> color_eyre::Result<()> {
        fs::write(&self.output_path, &content)?;
        Ok(())
    }
    fn build(template: String, params: CloudConfigParams) -> String {
        let mut generated = template.clone();
        let params = params.into_map();
        for (k, v) in params {
            // build search key '${KEY}'
            let key = format!("${{{k}}}");
            generated = generated.replace(&key, &v);
        }
        generated
    }
    pub fn generate(self, params: CloudConfigParams) -> color_eyre::Result<String> {
        let template = self.read_template()?;
        let output = Self::build(template, params);
        self.write_output(output)?;
        
        Ok(self.output_path)
    }
}

pub(crate) struct CloudConfigParams {
    pub vmid: u16,
    pub repo: String,
    pub labels: Vec<String>,
    pub runner_token: SecretString,
}
impl CloudConfigParams {
    fn into_map(self) -> HashMap<String,String> {
        let mut map = HashMap::new();
        map.insert("VMID".to_string(), self.vmid.to_string());
        map.insert("GH_REPO".to_string(), self.repo);
        map.insert("LABELS".to_string(), self.labels.join(","));
        map.insert("RUNNER_TOKEN".to_string(), self.runner_token.expose_secret().to_string());
        map
    }
}