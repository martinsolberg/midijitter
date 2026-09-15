use crate::AppError;

pub mod pipewire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiSource {
    pub display_name: String,
    pub node_name: String,
    pub port_name: String,
    pub object_serial: Option<String>,
    pub node_id: u32,
    pub port_id: u32,
}

impl MidiSource {
    pub fn stable_identity(&self) -> String {
        match &self.object_serial {
            Some(serial) => format!("{}/{}#{serial}", self.node_name, self.port_name),
            None => format!("{}/{}", self.node_name, self.port_name),
        }
    }
}

pub trait CaptureBackend {
    fn enumerate(&self) -> Result<Vec<MidiSource>, AppError>;
}

pub fn select_source(sources: &[MidiSource], selector: &str) -> Result<MidiSource, AppError> {
    let matches: Vec<_> = sources
        .iter()
        .filter(|source| source.display_name == selector || source.stable_identity() == selector)
        .cloned()
        .collect();

    match matches.as_slice() {
        [] if sources.is_empty() => Err(AppError::NoSource),
        [] => Err(AppError::SourceNotFound {
            selector: selector.to_owned(),
        }),
        [source] => Ok(source.clone()),
        _ => Err(AppError::AmbiguousSource {
            selector: selector.to_owned(),
            candidates: matches.iter().map(MidiSource::stable_identity).collect(),
        }),
    }
}
