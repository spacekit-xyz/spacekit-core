//! Bridge `LogEvent` ↔ SRA / SwtchVM structured logs.

use crate::service::{
    classify_sra_topic, topic_label_bytes, ServiceEvent, SraSwtchvmLog, FIELD_RESOURCE_UNITS,
};
use crate::{EventKind, FieldValue, LogEvent};

impl LogEvent {
    /// If this is a `Service` event, return the canonical SRA topic label.
    pub fn sra_topic_label(&self) -> Option<&'static str> {
        match &self.kind {
            EventKind::Service(e) => Some(e.sra_topic_label()),
            _ => None,
        }
    }

    /// Resource units for SRA weighting (`resource_units` field, required on Service events).
    pub fn sra_resource_units(&self) -> Option<u128> {
        match self.get_field(FIELD_RESOURCE_UNITS)? {
            FieldValue::Unsigned(u) => Some(*u as u128),
            FieldValue::Integer(i) if *i >= 0 => Some(*i as u128),
            _ => None,
        }
    }

    /// SwtchVM log parts for block inclusion / SRA ingestion.
    pub fn to_sra_swtchvm_log(&self) -> Option<SraSwtchvmLog> {
        let service = match &self.kind {
            EventKind::Service(e) => *e,
            _ => return None,
        };
        let units = self.sra_resource_units().unwrap_or(1);
        Some(SraSwtchvmLog::new(service, units))
    }

    /// Classify a raw SwtchVM topic0 (32-byte padded label) into a service event.
    pub fn service_event_from_topic0(topic0: &[u8; 32]) -> Option<ServiceEvent> {
        let end = topic0.iter().position(|&b| b == 0).unwrap_or(32);
        if end == 0 {
            return None;
        }
        let label = core::str::from_utf8(&topic0[..end]).ok()?;
        classify_sra_topic(label)
    }
}

/// Encode topic0 from any known label (canonical or legacy).
pub fn topic0_from_label(label: &str) -> [u8; 32] {
    topic_label_bytes(label)
}
