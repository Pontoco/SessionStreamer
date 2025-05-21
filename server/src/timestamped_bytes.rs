use std::{collections::VecDeque, io::BufRead, time::SystemTime};
use thiserror::Error;

use bytes::Bytes;
use chrono::{DateTime, Utc};


/// Decodes our timestamped bytestream format into UTF8 lines, each prepended with the timestamp when that line was first encountered.
pub struct TimestampedUtf8Depacketizer {
    pub holdover_timestamp: Option<String>, // None when no bytes have yet been received.
    pub utf8_bytes: VecDeque<u8>,
}

#[derive(Error, Debug)] // Added Debug for easier error handling
pub enum TsBytesError {
    #[error("Invalid packet.")]
    InvalidPacket, // E.g., too short to contain a timestamp
}

impl TimestampedUtf8Depacketizer {
    pub fn new() -> Self {
        Self {
            holdover_timestamp: None,
            utf8_bytes: VecDeque::new(),
        }
    }

    /// Processes an incoming packet.
    ///
    /// Extracts a timestamp from the beginning of the packet and appends the
    /// remaining bytes to an internal buffer. It then attempts to parse
    /// complete UTF-8 lines (ending with '\n') from this buffer.
    ///
    /// Returns a `Result` containing a `Vec<String>` of all complete lines
    /// extracted during this call. Bytes forming partial lines or any invalid
    /// UTF-8 sequences remain in the internal buffer for subsequent calls.
    ///
    /// The `last_timestamp` of the depacketizer is updated to the timestamp
    /// of the current packet after its payload has been processed.
    pub fn write_packet(&mut self, packet: Bytes) -> Result<Vec<String>, TsBytesError> {
        // Extract the first 8 bytes as a timestamp (nanos since unix epoch)
        // Send the remaining bytes into the utf8 stream.
        if packet.len() < 8 {
            return Err(TsBytesError::InvalidPacket);
        }

        let (timestamp_bytes, partial_utf8) = packet.split_at(8);

        // Convert the timestamp bytes to a u64.
        // Using try_into().map_err(...) would be more robust than unwrap()
        // For simplicity, keeping unwrap as in the original snippet.
        let timestamp_array: [u8; 8] = timestamp_bytes.try_into().map_err(|_| TsBytesError::InvalidPacket)?;
        let timestamp_nanos = u64::from_le_bytes(timestamp_array);
        let latest_timestamp: DateTime<Utc> = (SystemTime::UNIX_EPOCH + std::time::Duration::from_nanos(timestamp_nanos)).into();
        let timestamp_str = latest_timestamp.to_rfc3339();

        // Add the new bytes to our buffer
        self.utf8_bytes.extend(partial_utf8);

        let mut lines_found = Vec::new();

        loop {
            // Find the position of the next newline character ('\n').
            // We need to make the deque contiguous to reliably use iter().position()
            // or manually handle the two slices if not contiguous.
            // For simplicity in this loop, make_contiguous is fine,
            // but could be optimized if performance is critical by working with both slices.
            self.utf8_bytes.make_contiguous();
            let (current_buffer_slice, _) = self.utf8_bytes.as_slices();

            if current_buffer_slice.is_empty() {
                break;
            }

            let newline_pos = current_buffer_slice.iter().position(|&b| b == b'\n');

            if let Some(pos) = newline_pos {
                let line_len_with_newline = pos + 1;

                // Extract the potential line bytes (including newline)
                // Cloning here to attempt UTF-8 conversion.
                let potential_line_segment: Vec<u8> = current_buffer_slice[..line_len_with_newline].to_vec();

                let mut line_str = String::from_utf8_lossy(&potential_line_segment).into_owned();

                // Successfully decoded a UTF-8 string up to and including the newline.
                // Remove the trailing newline character.
                if line_str.ends_with('\n') {
                    line_str.pop();
                    // Optionally, also remove a carriage return if present (for \r\n endings)
                    if line_str.ends_with('\r') {
                        line_str.pop();
                    }
                }

                // Prepend the timestamp. Take the holdover timestamp if one was kept around from an unfinished previous line.
                let ts_str = self.holdover_timestamp.take().unwrap_or(timestamp_str.clone());
                let line_str = format!("{} {}", ts_str, line_str);

                lines_found.push(line_str);

                // Remove the processed line (including newline) from the deque.
                self.utf8_bytes.drain(..line_len_with_newline);
                // Continue to look for more lines in the buffer.
            } else {
                if let None = self.holdover_timestamp {
                    self.holdover_timestamp = Some(timestamp_str);
                }
                // No newline found in the rest of the buffer.
                // Any remaining data is a partial line. Leave it.
                break;
            }
        }

        Ok(lines_found)
    }
}

impl Default for TimestampedUtf8Depacketizer {
    fn default() -> Self {
        Self::new()
    }
}

// Example Usage (you can put this in a test module or main function)
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_packet(timestamp_nanos: u64, data: &[u8]) -> Bytes {
        let mut packet_data = Vec::new();
        packet_data.extend_from_slice(&timestamp_nanos.to_le_bytes());
        packet_data.extend_from_slice(data);
        Bytes::from(packet_data)
    }

    #[test]
    fn test_simple_line() {
        let mut depacketizer = TimestampedUtf8Depacketizer::new();
        let packet = create_packet(1000, b"hello\n");
        let lines = depacketizer.write_packet(packet).unwrap();
        assert_eq!(lines, vec!["1970-01-01T00:00:00.000001+00:00 hello"]);
        assert!(depacketizer.utf8_bytes.is_empty());
    }

    #[test]
    fn test_multiple_lines_in_one_packet() {
        let mut depacketizer = TimestampedUtf8Depacketizer::new();
        let packet = create_packet(2000, b"line1\nline2\n");
        let lines = depacketizer.write_packet(packet).unwrap();
        assert_eq!(lines, vec!["1970-01-01T00:00:00.000002+00:00 line1", "1970-01-01T00:00:00.000002+00:00 line2"]);
        assert!(depacketizer.utf8_bytes.is_empty());
    }

    #[test]
    fn test_partial_line_then_completion() {
        let mut depacketizer = TimestampedUtf8Depacketizer::new();
        let packet1 = create_packet(3000, b"part1...");
        let lines1 = depacketizer.write_packet(packet1).unwrap();
        assert!(lines1.is_empty());
        assert_eq!(depacketizer.utf8_bytes.iter().cloned().collect::<Vec<_>>(), b"part1...".to_vec());

        let packet2 = create_packet(4000, b"part2\nnextline\n");
        let lines2 = depacketizer.write_packet(packet2).unwrap();
        assert_eq!(lines2, vec!["1970-01-01T00:00:00.000003+00:00 part1...part2", "1970-01-01T00:00:00.000004+00:00 nextline"]);
        assert!(depacketizer.utf8_bytes.is_empty());
    }

    #[test]
    fn test_invalid_utf8_in_line() {
        let mut depacketizer = TimestampedUtf8Depacketizer::new();
        // Valid line, then invalid sequence before a newline
        let packet = create_packet(5000, b"valid line\nhello\xC3\x28world\nfinal line\n");
        // 0xC3 0x28 is an invalid UTF-8 sequence

        let lines = depacketizer.write_packet(packet).unwrap();
        assert_eq!(lines, vec!["1970-01-01T00:00:00.000005+00:00 valid line", "1970-01-01T00:00:00.000005+00:00 hello�(world", "1970-01-01T00:00:00.000005+00:00 final line"]); 

        assert!(depacketizer.utf8_bytes.is_empty());
    }

    #[test]
    fn test_invalid_utf8_at_start_of_packet_payload() {
        let mut depacketizer = TimestampedUtf8Depacketizer::new();
        let packet = create_packet(6000, b"\xFF\xFE");
        let lines = depacketizer.write_packet(packet).unwrap();
        assert!(lines.is_empty()); 

        let expected_remaining: Vec<u8> = b"\xFF\xFE".to_vec();
        depacketizer.utf8_bytes.make_contiguous();
        assert_eq!(depacketizer.utf8_bytes.as_slices().0, expected_remaining.as_slice());
    }

    #[test]
    fn test_empty_payload() {
        let mut depacketizer = TimestampedUtf8Depacketizer::new();
        let packet = create_packet(7000, b"");
        let lines = depacketizer.write_packet(packet).unwrap();
        assert!(lines.is_empty());
        assert!(depacketizer.utf8_bytes.is_empty());
    }

    #[test]
    fn test_invalid_packet_too_short() {
        let mut depacketizer = TimestampedUtf8Depacketizer::new();
        let packet_data = vec![1, 2, 3]; // Less than 8 bytes
        let packet = Bytes::from(packet_data);
        let result = depacketizer.write_packet(packet);
        assert!(matches!(result, Err(TsBytesError::InvalidPacket)));
    }
}

/// Reads as much UTF-8 as possible from the front of a `VecDeque<u8>`.
///
/// Returns a `String` containing the valid UTF-8 found at the beginning
/// of the deque. The processed valid UTF-8 bytes are removed from the deque.
/// Any remaining bytes (which would be the start of an invalid UTF-8 sequence
/// or an incomplete multi-byte sequence if the deque ends abruptly) are left
/// in the `deque`.
///
/// # Examples
///
/// ```
/// use std::collections::VecDeque;
/// // Assuming the function `read_utf8_from_deque` is defined:
/// fn read_utf8_from_deque(deque: &mut VecDeque<u8>) -> String {
///     if deque.is_empty() {
///         return String::new();
///     }
///     deque.make_contiguous(); // Ensure easier slicing
///     let (slice1, _) = deque.as_slices(); // After make_contiguous, data is in slice1
///
///     let mut valid_utf8_len = 0;
///     match std::str::from_utf8(slice1) {
///         Ok(_) => {
///             // The entire slice is valid UTF-8
///             valid_utf8_len = slice1.len();
///         }
///         Err(e) => {
///             // The error tells us how many bytes were valid *before* the error.
///             valid_utf8_len = e.valid_up_to();
///         }
///     }
///
///     if valid_utf8_len > 0 {
///         let result_bytes: Vec<u8> = deque.drain(0..valid_utf8_len).collect();
///         // This conversion should not fail as we've already validated this exact prefix.
///         String::from_utf8(result_bytes).unwrap_or_else(|_| {
///             // This fallback should ideally not be reached if logic is correct
///             String::new()
///         })
///     } else {
///         String::new()
///     }
/// }
///
/// let mut deque1: VecDeque<u8> = VecDeque::from(vec![
///     0xE4, 0xBD, 0xA0, // 你 (ni)
///     0xE5, 0xA5, 0xBD, // 好 (hao)
///     0xC3, 0x28,       // Invalid UTF-8 sequence (0xC3 followed by 0x28)
///     0x61,
/// ]);
/// let result1 = read_utf8_from_deque(&mut deque1);
/// assert_eq!(result1, "你好");
/// assert_eq!(deque1, VecDeque::from(vec![0xC3, 0x28, 0x61]));
///
/// let mut deque2: VecDeque<u8> = VecDeque::from(vec![0x68, 0x65, 0x6C, 0x6C, 0x6F]); // "hello"
/// let result2 = read_utf8_from_deque(&mut deque2);
/// assert_eq!(result2, "hello");
/// assert!(deque2.is_empty());
///
/// let mut deque3: VecDeque<u8> = VecDeque::from(vec![0xFF, 0xFE]); // Invalid start
/// let result3 = read_utf8_from_deque(&mut deque3);
/// assert_eq!(result3, "");
/// assert_eq!(deque3, VecDeque::from(vec![0xFF, 0xFE]));
///
/// let mut deque4: VecDeque<u8> = VecDeque::from(vec![0xE4, 0xBD]); // Incomplete sequence
/// let result4 = read_utf8_from_deque(&mut deque4);
/// // from_utf8 would error, valid_up_to would be 0 for an incomplete sequence at the end
/// assert_eq!(result4, "");
/// assert_eq!(deque4, VecDeque::from(vec![0xE4, 0xBD]));
///
/// // If the incomplete sequence is followed by something, valid_up_to handles it.
/// let mut deque5: VecDeque<u8> = VecDeque::from(vec![0xE4, /* missing BD */ 0x61]);
/// let result5 = read_utf8_from_deque(&mut deque5);
/// assert_eq!(result5, ""); // 0xE4 is not valid alone, valid_up_to is 0
/// assert_eq!(deque5, VecDeque::from(vec![0xE4, 0x61]));
///
/// let mut deque6: VecDeque<u8> = VecDeque::from(vec![0x61, 0xE4, 0xBD, /* missing A0 */ 0x62]);
/// let result6 = read_utf8_from_deque(&mut deque6);
/// assert_eq!(result6, "a"); // 'a' is valid. 0xE4 0xBD is incomplete. valid_up_to for [0x61, 0xE4, 0xBD, 0x62] is 1.
/// assert_eq!(deque6, VecDeque::from(vec![0xE4, 0xBD, 0x62]));
///
/// ```
pub fn read_utf8_from_deque(deque: &mut VecDeque<u8>) -> String {
    if deque.is_empty() {
        return String::new();
    }

    // Make the deque contiguous for straightforward slicing.
    // This might involve a data copy if the deque's buffer has wrapped.
    deque.make_contiguous();
    let (slice1, slice2_must_be_empty) = deque.as_slices();

    // After make_contiguous, all data is in slice1. slice2_must_be_empty should be empty.
    debug_assert!(slice2_must_be_empty.is_empty());

    // Attempt to interpret the entire current buffer as UTF-8
    let valid_utf8_len = match std::str::from_utf8(slice1) {
        Ok(_) => {
            // The entire buffer is valid UTF-8
            slice1.len()
        }
        Err(e) => {
            // An error occurred. `e.valid_up_to()` gives the number of bytes
            // from the start of the slice that were valid before the error.
            // This also correctly handles cases where an incomplete sequence
            // is at `slice1[e.valid_up_to()..]`.
            e.valid_up_to()
        }
    };

    if valid_utf8_len > 0 {
        // Drain the valid UTF-8 bytes from the front of the deque
        let result_bytes: Vec<u8> = deque.drain(0..valid_utf8_len).collect();

        // This conversion must succeed as we've determined these exact bytes are valid UTF-8.
        String::from_utf8(result_bytes).expect("UTF-8 conversion failed despite pre-validation; this is a bug.")
    } else {
        // No valid UTF-8 sequence found at the beginning.
        String::new()
    }
}
