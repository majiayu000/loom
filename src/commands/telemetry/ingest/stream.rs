use std::io::BufRead;

use crate::types::ErrorCode;

use super::super::super::CommandFailure;
use super::super::super::helpers::map_io;

pub(super) const MAX_RECORD_BYTES: usize = 1024 * 1024;

pub(super) enum RecordStatus {
    Complete { consumed: u64 },
    Oversized { consumed: u64 },
    Partial { consumed: u64 },
    Eof,
}

pub(super) fn read_record(
    reader: &mut impl BufRead,
    record: &mut Vec<u8>,
) -> std::result::Result<RecordStatus, CommandFailure> {
    record.clear();
    let mut consumed_total = 0u64;
    let mut oversized = false;
    loop {
        let available = reader.fill_buf().map_err(map_io)?;
        if available.is_empty() {
            return if consumed_total == 0 {
                Ok(RecordStatus::Eof)
            } else {
                Ok(RecordStatus::Partial {
                    consumed: consumed_total,
                })
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let content = newline.unwrap_or(available.len());
        if !oversized && record.len().saturating_add(content) > MAX_RECORD_BYTES {
            oversized = true;
            record.clear();
        }
        if !oversized {
            record.extend_from_slice(&available[..content]);
        }
        reader.consume(consumed);
        consumed_total = consumed_total
            .checked_add(u64::try_from(consumed).map_err(|_| {
                CommandFailure::new(
                    ErrorCode::InternalError,
                    "telemetry ingest record offset overflow",
                )
            })?)
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::InternalError,
                    "telemetry ingest record offset overflow",
                )
            })?;
        if newline.is_some() {
            return Ok(if oversized {
                RecordStatus::Oversized {
                    consumed: consumed_total,
                }
            } else {
                RecordStatus::Complete {
                    consumed: consumed_total,
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::{MAX_RECORD_BYTES, RecordStatus, read_record};

    #[test]
    fn reads_across_small_buffers_and_preserves_partial_tail() {
        let mut reader = BufReader::with_capacity(3, &b"first\npartial"[..]);
        let mut record = Vec::new();
        assert!(matches!(
            read_record(&mut reader, &mut record).expect("complete record"),
            RecordStatus::Complete { consumed: 6 }
        ));
        assert_eq!(record, b"first");
        assert!(matches!(
            read_record(&mut reader, &mut record).expect("partial record"),
            RecordStatus::Partial { consumed: 7 }
        ));
        assert_eq!(record, b"partial");
    }

    #[test]
    fn skips_oversized_record_before_unbounded_growth() {
        let mut input = vec![b'x'; MAX_RECORD_BYTES + 1];
        input.push(b'\n');
        let mut reader = BufReader::with_capacity(4096, input.as_slice());
        let mut record = Vec::new();
        assert!(matches!(
            read_record(&mut reader, &mut record).expect("bounded skip"),
            RecordStatus::Oversized { .. }
        ));
        assert!(record.len() <= MAX_RECORD_BYTES);
    }
}
