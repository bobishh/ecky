use crate::contracts::{GeometryBackend, SourceLanguage};

pub fn authored_source_file_name(
    source_language: SourceLanguage,
    _geometry_backend: GeometryBackend,
) -> &'static str {
    match source_language {
        SourceLanguage::LegacyPython => "source.FCMacro",
        SourceLanguage::Build123d => "source.py",
        SourceLanguage::EckyIrV0 => "source.ecky",
    }
}

pub fn lowered_source_file_name(geometry_backend: GeometryBackend) -> &'static str {
    match geometry_backend {
        GeometryBackend::Build123d => "lowered.py",
        GeometryBackend::Freecad => "lowered.FCMacro",
        GeometryBackend::EckyRust => "source.ecky",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_source_names_keep_single_ecky_extension() {
        assert_eq!(
            authored_source_file_name(SourceLanguage::EckyIrV0, GeometryBackend::Build123d),
            "source.ecky"
        );
        assert_eq!(
            authored_source_file_name(SourceLanguage::EckyIrV0, GeometryBackend::Freecad),
            "source.ecky"
        );
        assert_eq!(
            authored_source_file_name(SourceLanguage::EckyIrV0, GeometryBackend::EckyRust),
            "source.ecky"
        );
        assert_eq!(
            authored_source_file_name(SourceLanguage::Build123d, GeometryBackend::Build123d),
            "source.py"
        );
        assert_eq!(
            authored_source_file_name(SourceLanguage::LegacyPython, GeometryBackend::Freecad),
            "source.FCMacro"
        );
    }

    #[test]
    fn lowered_source_names_stay_backend_specific() {
        assert_eq!(
            lowered_source_file_name(GeometryBackend::Build123d),
            "lowered.py"
        );
        assert_eq!(
            lowered_source_file_name(GeometryBackend::Freecad),
            "lowered.FCMacro"
        );
    }
}
