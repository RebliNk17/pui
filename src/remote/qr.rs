//! Pure-Rust ISO/IEC 18004 QR Code encoder (Byte mode, ECC Level L, Versions 1..=10).
//! Used to render a scannable pairing QR code inside Pui Settings without extra Cargo crates.

/// A square QR code matrix (`true` = dark module, `false` = light module).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrMatrix {
    pub size: usize,
    pub modules: Vec<bool>,
}

impl QrMatrix {
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.size + x]
    }
}

struct VersionSpec {
    version: usize,
    data_codewords: usize,
    ec_per_block: usize,
    g1_blocks: usize,
    g1_data: usize,
    g2_blocks: usize,
    g2_data: usize,
    alignments: &'static [usize],
}

// ISO/IEC 18004 Table 9 (Error Correction Level L) for Versions 1..=10.
const VERSIONS_L: [VersionSpec; 10] = [
    VersionSpec {
        version: 1,
        data_codewords: 19,
        ec_per_block: 7,
        g1_blocks: 1,
        g1_data: 19,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[],
    },
    VersionSpec {
        version: 2,
        data_codewords: 34,
        ec_per_block: 10,
        g1_blocks: 1,
        g1_data: 34,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 18],
    },
    VersionSpec {
        version: 3,
        data_codewords: 55,
        ec_per_block: 15,
        g1_blocks: 1,
        g1_data: 55,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 22],
    },
    VersionSpec {
        version: 4,
        data_codewords: 80,
        ec_per_block: 20,
        g1_blocks: 1,
        g1_data: 80,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 26],
    },
    VersionSpec {
        version: 5,
        data_codewords: 108,
        ec_per_block: 26,
        g1_blocks: 1,
        g1_data: 108,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 30],
    },
    VersionSpec {
        version: 6,
        data_codewords: 136,
        ec_per_block: 18,
        g1_blocks: 2,
        g1_data: 68,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 34],
    },
    VersionSpec {
        version: 7,
        data_codewords: 156,
        ec_per_block: 20,
        g1_blocks: 2,
        g1_data: 78,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 22, 38],
    },
    VersionSpec {
        version: 8,
        data_codewords: 194,
        ec_per_block: 24,
        g1_blocks: 2,
        g1_data: 97,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 24, 42],
    },
    VersionSpec {
        version: 9,
        data_codewords: 232,
        ec_per_block: 30,
        g1_blocks: 2,
        g1_data: 116,
        g2_blocks: 0,
        g2_data: 0,
        alignments: &[6, 26, 46],
    },
    VersionSpec {
        version: 10,
        data_codewords: 274,
        ec_per_block: 18,
        g1_blocks: 2,
        g1_data: 68,
        g2_blocks: 2,
        g2_data: 69,
        alignments: &[6, 28, 50],
    },
];

// Version information bitstrings for versions 7..=10 (18 bits each, Golay(18,6)).
const VERSION_INFO: [u32; 4] = [0x07C94, 0x085BC, 0x09A99, 0x0A4D3];

// Precomputed 15-bit format strings for Level L (01) and masks 0..=7 (XORed with 0x5412).
const FORMAT_INFO_L: [u16; 8] = [
    0x77C4, 0x72F3, 0x7DAA, 0x789D, 0x662F, 0x6318, 0x6C41, 0x6976,
];

pub fn encode(text: &str) -> Option<QrMatrix> {
    let bytes = text.as_bytes();
    let spec = VERSIONS_L.iter().find(|spec| {
        let count_bits = if spec.version <= 9 { 8 } else { 16 };
        let needed_bits = 4 + count_bits + bytes.len() * 8;
        needed_bits <= spec.data_codewords * 8
    })?;

    let mut bits = Vec::with_capacity(spec.data_codewords * 8);
    push_bits(&mut bits, 0b0100, 4); // Byte mode
    let count_bits = if spec.version <= 9 { 8 } else { 16 };
    push_bits(&mut bits, bytes.len() as u32, count_bits);
    for &b in bytes {
        push_bits(&mut bits, b as u32, 8);
    }

    let total_data_bits = spec.data_codewords * 8;
    let terminator = (total_data_bits - bits.len()).min(4);
    push_bits(&mut bits, 0, terminator);
    while bits.len() % 8 != 0 {
        bits.push(false);
    }

    let mut data_codewords = Vec::with_capacity(spec.data_codewords);
    for chunk in bits.chunks_exact(8) {
        let mut byte = 0u8;
        for &bit in chunk {
            byte = (byte << 1) | u8::from(bit);
        }
        data_codewords.push(byte);
    }
    let mut pad_toggle = false;
    while data_codewords.len() < spec.data_codewords {
        data_codewords.push(if pad_toggle { 0x11 } else { 0xEC });
        pad_toggle = !pad_toggle;
    }

    // Split into RS blocks, compute EC codewords, and interleave.
    let mut data_blocks: Vec<Vec<u8>> = Vec::new();
    let mut ec_blocks: Vec<Vec<u8>> = Vec::new();
    let mut offset = 0;
    for _ in 0..spec.g1_blocks {
        let block = data_codewords[offset..offset + spec.g1_data].to_vec();
        offset += spec.g1_data;
        ec_blocks.push(rs_encode(&block, spec.ec_per_block));
        data_blocks.push(block);
    }
    for _ in 0..spec.g2_blocks {
        let block = data_codewords[offset..offset + spec.g2_data].to_vec();
        offset += spec.g2_data;
        ec_blocks.push(rs_encode(&block, spec.ec_per_block));
        data_blocks.push(block);
    }

    let mut interleaved = Vec::new();
    let max_data_len = data_blocks.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..max_data_len {
        for block in &data_blocks {
            if let Some(&b) = block.get(i) {
                interleaved.push(b);
            }
        }
    }
    for i in 0..spec.ec_per_block {
        for block in &ec_blocks {
            interleaved.push(block[i]);
        }
    }

    let size = spec.version * 4 + 17;
    let mut modules = vec![false; size * size];
    let mut is_function = vec![false; size * size];

    place_finder(&mut modules, &mut is_function, size, 0, 0);
    place_finder(&mut modules, &mut is_function, size, size - 7, 0);
    place_finder(&mut modules, &mut is_function, size, 0, size - 7);

    // Timing patterns.
    for i in 8..size - 8 {
        let dark = i % 2 == 0;
        set_fn(&mut modules, &mut is_function, size, i, 6, dark);
        set_fn(&mut modules, &mut is_function, size, 6, i, dark);
    }

    // Alignment patterns.
    for &ay in spec.alignments {
        for &ax in spec.alignments {
            let overlaps_finder =
                (ax <= 8 && ay <= 8) || (ax >= size - 9 && ay <= 8) || (ax <= 8 && ay >= size - 9);
            if !overlaps_finder {
                place_alignment(&mut modules, &mut is_function, size, ax, ay);
            }
        }
    }

    // Dark module.
    set_fn(&mut modules, &mut is_function, size, 8, size - 8, true);

    // Reserve format information areas.
    for i in 0..=8 {
        if i != 6 {
            is_function[8 * size + i] = true;
            is_function[i * size + 8] = true;
        }
    }
    for i in 0..8 {
        is_function[8 * size + (size - 1 - i)] = true;
    }
    for i in 0..7 {
        is_function[(size - 1 - i) * size + 8] = true;
    }

    // Version info for versions 7..=10.
    if spec.version >= 7 {
        let vbits = VERSION_INFO[spec.version - 7];
        for i in 0..18 {
            let dark = ((vbits >> i) & 1) != 0;
            let row = i / 3;
            let col = size - 11 + (i % 3);
            set_fn(&mut modules, &mut is_function, size, col, row, dark);
            set_fn(&mut modules, &mut is_function, size, row, col, dark);
        }
    }

    // Place interleaved data + EC bits in zigzag column pairs.
    let mut bit_idx = 0usize;
    let total_bits = interleaved.len() * 8;
    let mut right = size as isize - 1;
    let mut upward = true;
    while right > 0 {
        if right == 6 {
            right -= 1;
        }
        for step in 0..size {
            let y = if upward { size - 1 - step } else { step };
            for dx in [0isize, -1] {
                let x = (right + dx) as usize;
                let idx = y * size + x;
                if !is_function[idx] {
                    let dark = if bit_idx < total_bits {
                        let byte = interleaved[bit_idx / 8];
                        ((byte >> (7 - (bit_idx % 8))) & 1) != 0
                    } else {
                        false
                    };
                    modules[idx] = dark;
                    bit_idx += 1;
                }
            }
        }
        right -= 2;
        upward = !upward;
    }

    // Evaluate all 8 masks and pick the lowest penalty.
    let mut best_modules = modules.clone();
    let mut best_penalty = u32::MAX;
    for (mask, &format_bits) in FORMAT_INFO_L.iter().enumerate() {
        let mut candidate = modules.clone();
        for y in 0..size {
            for x in 0..size {
                let idx = y * size + x;
                if !is_function[idx] && mask_bit(mask, x, y) {
                    candidate[idx] = !candidate[idx];
                }
            }
        }
        write_format_info(&mut candidate, size, format_bits);
        let score = penalty(&candidate, size);
        if score < best_penalty {
            best_penalty = score;
            best_modules = candidate;
        }
    }

    Some(QrMatrix {
        size,
        modules: best_modules,
    })
}

fn push_bits(out: &mut Vec<bool>, value: u32, count: usize) {
    for i in (0..count).rev() {
        out.push(((value >> i) & 1) != 0);
    }
}

fn set_fn(modules: &mut [bool], is_fn: &mut [bool], size: usize, x: usize, y: usize, dark: bool) {
    let idx = y * size + x;
    modules[idx] = dark;
    is_fn[idx] = true;
}

fn place_finder(modules: &mut [bool], is_fn: &mut [bool], size: usize, x0: usize, y0: usize) {
    for dy in -1isize..=7 {
        for dx in -1isize..=7 {
            let x = x0 as isize + dx;
            let y = y0 as isize + dy;
            if x < 0 || y < 0 || x >= size as isize || y >= size as isize {
                continue;
            }
            let in_finder = (0..=6).contains(&dx) && (0..=6).contains(&dy);
            let dark = in_finder
                && (dx == 0
                    || dx == 6
                    || dy == 0
                    || dy == 6
                    || ((2..=4).contains(&dx) && (2..=4).contains(&dy)));
            set_fn(modules, is_fn, size, x as usize, y as usize, dark);
        }
    }
}

fn place_alignment(modules: &mut [bool], is_fn: &mut [bool], size: usize, cx: usize, cy: usize) {
    for dy in -2isize..=2 {
        for dx in -2isize..=2 {
            let dist = dx.abs().max(dy.abs());
            let dark = dist != 1;
            set_fn(
                modules,
                is_fn,
                size,
                (cx as isize + dx) as usize,
                (cy as isize + dy) as usize,
                dark,
            );
        }
    }
}

fn mask_bit(mask: usize, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y) % 2 == 0,
        1 => y % 2 == 0,
        2 => x % 3 == 0,
        3 => (x + y) % 3 == 0,
        4 => ((y / 2) + (x / 3)) % 2 == 0,
        5 => ((x * y) % 2) + ((x * y) % 3) == 0,
        6 => (((x * y) % 2) + ((x * y) % 3)) % 2 == 0,
        _ => (((x + y) % 2) + ((x * y) % 3)) % 2 == 0,
    }
}

fn write_format_info(modules: &mut [bool], size: usize, bits: u16) {
    let bit = |i: usize| ((bits >> i) & 1) != 0;
    // Top-left horizontal & vertical.
    let coords1 = [
        (0, 8),
        (1, 8),
        (2, 8),
        (3, 8),
        (4, 8),
        (5, 8),
        (7, 8),
        (8, 8),
        (8, 7),
        (8, 5),
        (8, 4),
        (8, 3),
        (8, 2),
        (8, 1),
        (8, 0),
    ];
    for (idx, &(x, y)) in coords1.iter().enumerate() {
        modules[y * size + x] = bit(14 - idx);
    }
    // Bottom-left vertical (bits 14..8) & top-right horizontal (bits 7..0).
    for i in 0..7 {
        modules[(size - 1 - i) * size + 8] = bit(14 - i);
    }
    for i in 0..8 {
        modules[8 * size + (size - 8 + i)] = bit(7 - i);
    }
}

fn penalty(modules: &[bool], size: usize) -> u32 {
    let mut score = 0u32;
    // Rule 1: runs of 5+ identical modules in rows and columns.
    for y in 0..size {
        let mut run = 1u32;
        for x in 1..size {
            if modules[y * size + x] == modules[y * size + x - 1] {
                run += 1;
                if run == 5 {
                    score += 3;
                } else if run > 5 {
                    score += 1;
                }
            } else {
                run = 1;
            }
        }
    }
    for x in 0..size {
        let mut run = 1u32;
        for y in 1..size {
            if modules[y * size + x] == modules[(y - 1) * size + x] {
                run += 1;
                if run == 5 {
                    score += 3;
                } else if run > 5 {
                    score += 1;
                }
            } else {
                run = 1;
            }
        }
    }
    // Rule 2: 2x2 blocks of the same color.
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let c = modules[y * size + x];
            if c == modules[y * size + x + 1]
                && c == modules[(y + 1) * size + x]
                && c == modules[(y + 1) * size + x + 1]
            {
                score += 3;
            }
        }
    }
    score
}

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut res = 0u8;
    for _ in 0..8 {
        if (b & 1) != 0 {
            res ^= a;
        }
        let hi = (a & 0x80) != 0;
        a <<= 1;
        if hi {
            a ^= 0x1D; // Primitive polynomial x^8 + x^4 + x^3 + x^2 + 1 (0x11D).
        }
        b >>= 1;
    }
    res
}

fn rs_generator(degree: usize) -> Vec<u8> {
    let mut poly = vec![1u8];
    let mut root = 1u8;
    for _ in 0..degree {
        let mut next = vec![0u8; poly.len() + 1];
        for (i, &coeff) in poly.iter().enumerate() {
            next[i] ^= coeff;
            next[i + 1] ^= gf_mul(coeff, root);
        }
        poly = next;
        root = gf_mul(root, 2);
    }
    poly
}

fn rs_encode(data: &[u8], ec_len: usize) -> Vec<u8> {
    let generator = rs_generator(ec_len);
    let mut rem = vec![0u8; ec_len];
    for &byte in data {
        let factor = byte ^ rem[0];
        rem.rotate_left(1);
        rem[ec_len - 1] = 0;
        for i in 0..ec_len {
            rem[i] ^= gf_mul(generator[i + 1], factor);
        }
    }
    rem
}
