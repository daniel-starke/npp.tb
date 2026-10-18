//! Standard Base64 encoding (RFC 4648, with padding) for `data:` URIs.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PADDING: char = '=';

pub fn encoded_length(byte_count: usize) -> usize {
	byte_count.div_ceil(3) * 4
}

pub fn encode_into(bytes: &[u8], output: &mut String) {
	output.reserve(encoded_length(bytes.len()));
	let sextet = |group: u32, index: u32| ALPHABET[(group >> (18 - 6 * index) & 0x3F) as usize];
	let (chunks, rest) = bytes.as_chunks::<3>();
	for &[first, second, third] in chunks {
		let group = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);
		for index in 0..4 {
			output.push(char::from(sextet(group, index)));
		}
	}
	if rest.is_empty() {
		return;
	}
	let group = u32::from(rest[0]) << 16 | rest.get(1).map_or(0, |&byte| u32::from(byte) << 8);
	for index in 0..4 {
		if index <= rest.len() as u32 {
			output.push(char::from(sextet(group, index)));
		} else {
			output.push(PADDING);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn encode(bytes: &[u8]) -> String {
		let mut output = String::new();
		encode_into(bytes, &mut output);
		output
	}

	#[test]
	fn encodes_rfc_4648_test_vectors() {
		let vectors = [
			("", ""),
			("f", "Zg=="),
			("fo", "Zm8="),
			("foo", "Zm9v"),
			("foob", "Zm9vYg=="),
			("fooba", "Zm9vYmE="),
			("foobar", "Zm9vYmFy"),
		];
		for (input, expected) in vectors {
			assert_eq!(encode(input.as_bytes()), expected);
			assert_eq!(encoded_length(input.len()), expected.len());
		}
	}

	#[test]
	fn uses_the_whole_alphabet() {
		assert_eq!(encode(&[0xFB, 0xFF, 0xBF]), "+/+/");
		assert_eq!(encode(&[0x00, 0x00, 0x00]), "AAAA");
	}
}
