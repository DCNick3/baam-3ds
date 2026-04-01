static WIN1251_DATA: [u16; 128] = [
    0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409, 0x2039,
    0x040A, 0x040C, 0x040B, 0x040F, 0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x0098, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F, 0x00A0, 0x040E, 0x045E, 0x0408,
    0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407,
    0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB,
    0x0458, 0x0405, 0x0455, 0x0457, 0x0410, 0x0411, 0x0412, 0x0413, 0x0414, 0x0415, 0x0416, 0x0417,
    0x0418, 0x0419, 0x041A, 0x041B, 0x041C, 0x041D, 0x041E, 0x041F, 0x0420, 0x0421, 0x0422, 0x0423,
    0x0424, 0x0425, 0x0426, 0x0427, 0x0428, 0x0429, 0x042A, 0x042B, 0x042C, 0x042D, 0x042E, 0x042F,
    0x0430, 0x0431, 0x0432, 0x0433, 0x0434, 0x0435, 0x0436, 0x0437, 0x0438, 0x0439, 0x043A, 0x043B,
    0x043C, 0x043D, 0x043E, 0x043F, 0x0440, 0x0441, 0x0442, 0x0443, 0x0444, 0x0445, 0x0446, 0x0447,
    0x0448, 0x0449, 0x044A, 0x044B, 0x044C, 0x044D, 0x044E, 0x044F,
];
static CP866_DATA: [u16; 128] = [
    0x0410, 0x0411, 0x0412, 0x0413, 0x0414, 0x0415, 0x0416, 0x0417, 0x0418, 0x0419, 0x041A, 0x041B,
    0x041C, 0x041D, 0x041E, 0x041F, 0x0420, 0x0421, 0x0422, 0x0423, 0x0424, 0x0425, 0x0426, 0x0427,
    0x0428, 0x0429, 0x042A, 0x042B, 0x042C, 0x042D, 0x042E, 0x042F, 0x0430, 0x0431, 0x0432, 0x0433,
    0x0434, 0x0435, 0x0436, 0x0437, 0x0438, 0x0439, 0x043A, 0x043B, 0x043C, 0x043D, 0x043E, 0x043F,
    0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x2561, 0x2562, 0x2556, 0x2555, 0x2563, 0x2551, 0x2557,
    0x255D, 0x255C, 0x255B, 0x2510, 0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x255E, 0x255F,
    0x255A, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256C, 0x2567, 0x2568, 0x2564, 0x2565, 0x2559,
    0x2558, 0x2552, 0x2553, 0x256B, 0x256A, 0x2518, 0x250C, 0x2588, 0x2584, 0x258C, 0x2590, 0x2580,
    0x0440, 0x0441, 0x0442, 0x0443, 0x0444, 0x0445, 0x0446, 0x0447, 0x0448, 0x0449, 0x044A, 0x044B,
    0x044C, 0x044D, 0x044E, 0x044F, 0x0401, 0x0451, 0x0404, 0x0454, 0x0407, 0x0457, 0x040E, 0x045E,
    0x00B0, 0x2219, 0x00B7, 0x221A, 0x2116, 0x00A4, 0x25A0, 0x00A0,
];

pub const WIN1251_ENCODER: SingleByteEncoder = SingleByteEncoder {
    table: &WIN1251_DATA,
    run_bmp_offset: 0x0410,
    run_byte_offset: 64,
    run_length: 64,
};

pub const CP866_ENCODER: SingleByteEncoder = SingleByteEncoder {
    table: &CP866_DATA,
    run_bmp_offset: 0x0440,
    run_byte_offset: 96,
    run_length: 16,
};

#[inline(always)]
pub fn position(haystack: &[u16], needle: u16) -> Option<usize> {
    haystack.iter().position(|&x| x == needle)
}

pub struct SingleByteEncoder {
    table: &'static [u16; 128],
    run_bmp_offset: usize,
    run_byte_offset: usize,
    run_length: usize,
}

impl SingleByteEncoder {
    #[inline(always)]
    pub fn encode_char(&self, code_unit: char) -> Option<u8> {
        let code_unit = code_unit as u32;
        if code_unit > u16::MAX as u32 {
            // Out of BMP. Single-byte encodings do not correspond to codepoints outside of BMP.
            return None;
        }
        let code_unit = code_unit as u16;
        if code_unit < 128 {
            // ASCII
            return Some(code_unit as u8);
        }

        // First, we see if the code unit falls into a run of consecutive
        // code units that can be mapped by offset. This is very efficient
        // for most non-Latin encodings as well as Latin1-ish encodings.
        //
        // For encodings that don't fit this pattern, the run (which may
        // have the length of just one) just establishes the starting point
        // for the next rule.
        //
        // Next, we do a forward linear search in the part of the index
        // after the run. Even in non-Latin1-ish Latin encodings (except
        // macintosh), the lower case letters are here.
        //
        // Next, we search the third quadrant up to the start of the run
        // (upper case letters in Latin encodings except macintosh, in
        // Greek and in KOI encodings) and then the second quadrant,
        // except if the run stared before the third quadrant, we search
        // the second quadrant up to the run.
        //
        // Last, we search the first quadrant, which has unused controls
        // or punctuation in most encodings. This is bad for macintosh
        // and IBM866, but those are rare.

        // Run of consecutive units
        let unit_as_usize = code_unit as usize;
        let offset = unit_as_usize.wrapping_sub(self.run_bmp_offset);
        if offset < self.run_length {
            return Some((128 + self.run_byte_offset + offset) as u8);
        }

        // Search after the run
        let tail_start = self.run_byte_offset + self.run_length;
        if let Some(pos) = position(&self.table[tail_start..], code_unit) {
            return Some((128 + tail_start + pos) as u8);
        }

        if self.run_byte_offset >= 64 {
            // Search third quadrant before the run
            if let Some(pos) = position(&self.table[64..self.run_byte_offset], code_unit) {
                return Some(((128 + 64) + pos) as u8);
            }

            // Search second quadrant
            if let Some(pos) = position(&self.table[32..64], code_unit) {
                return Some(((128 + 32) + pos) as u8);
            }
        } else if let Some(pos) = position(&self.table[32..self.run_byte_offset], code_unit) {
            // windows-1252, windows-874, ISO-8859-15 and ISO-8859-5
            // Search second quadrant before the run
            return Some(((128 + 32) + pos) as u8);
        }

        // Search first quadrant
        if let Some(pos) = position(&self.table[..32], code_unit) {
            return Some((128 + pos) as u8);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        let enc = &super::WIN1251_ENCODER;

        assert_eq!(enc.encode_char('a'), Some(b'a'));
        assert_eq!(enc.encode_char('b'), Some(b'b'));
        assert_eq!(enc.encode_char('c'), Some(b'c'));
        assert_eq!(enc.encode_char('d'), Some(b'd'));

        assert_eq!(enc.encode_char('а'), Some(0xE0));
        assert_eq!(enc.encode_char('б'), Some(0xE1));
        assert_eq!(enc.encode_char('в'), Some(0xE2));
        assert_eq!(enc.encode_char('г'), Some(0xE3));
        assert_eq!(enc.encode_char('Ё'), Some(0xA8));
    }
}
