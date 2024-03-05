use std::mem::size_of;

use bincode::{config, error::EncodeError, Decode, Encode};
use crc::{Crc, CRC_32_CKSUM};
use image::{ColorType, EncodableLayout};
use rand::{thread_rng, Rng};

#[derive(Encode, Decode, PartialEq, Debug, Clone, Copy)]
pub(crate) enum V1DataStuffingOptions {
    None {
        /// How many pixels offset do we start?
        start_offset: u64,
    },
}

#[derive(Encode, Decode, PartialEq, Debug, Clone, Copy)]
pub(crate) enum VersionedHeader {
    V1 {
        stuffing_opts: V1DataStuffingOptions,
        /// A mask defining which bits inside a Pixel are used for data
        /// The length depends on the Color Type used.
        ///
        /// e.g `RGBA8` will have 8×4 -> 4 Bytes  
        /// Bits that are set are used for data
        data_mask: u64,
        /// Length of the payload
        ///
        /// This is NOT the count of pixels etc. This is the input/output length!
        data_len: u64,
    },
}

#[derive(Encode, Decode, PartialEq, Debug, Clone)]
pub(crate) struct HeaderRaw {
    /// Should be 0x42. Here so we can  
    pub(crate) magic: u8,
    /// How many bytes (=pixels*8) are used for the data segment
    pub(crate) header_len: u16,
    pub(crate) data: Vec<u8>,
    /// Checksum of the header and data
    pub(crate) crc: u32,
}

impl TryInto<HeaderRaw> for VersionedHeader {
    type Error = EncodeError;

    fn try_into(self) -> Result<HeaderRaw, Self::Error> {
        let data = bincode::encode_to_vec(self, config::standard())?;
        let crc = Crc::<u32>::new(&CRC_32_CKSUM).checksum(&data.as_bytes());

        Ok(HeaderRaw {
            magic: 0x42,
            header_len: data.len() as u16,
            data,
            crc,
        })
    }
}

impl TryFrom<HeaderRaw> for VersionedHeader {
    type Error = String;

    fn try_from(value: HeaderRaw) -> Result<Self, Self::Error> {
        if value.magic != 0x42 {
            return Err("Not a valid header: Magic Number is not 0x42".to_string());
        }

        // Check the checksum
        let crc = Crc::<u32>::new(&CRC_32_CKSUM).checksum(&value.data.as_bytes());
        if crc != value.crc {
            return Err(format!(
                "Checksum Mismatch. Expected {:#01x}, but found {:#01x}",
                crc, value.crc
            ));
        }

        // Try to parse Header from binary data
        let (payload, _): (VersionedHeader, _) =
            bincode::decode_from_slice(&value.data.as_slice(), config::standard())
                .map_err(|x| format!("Failed to decode header payload: {}", x))?;

        Ok(payload)
    }
}

///
/// This function basically determines the u64 which acts as a data mask
///
fn calculate_bit_mask(bits_needed_per_pixel: u8, color_type: ColorType) -> u64 {
    let bit_count_on_all_channels = bits_needed_per_pixel / color_type.channel_count();
    let mut data_bits_per_channel: Vec<usize> =
        vec![bit_count_on_all_channels as usize; color_type.channel_count() as usize];

    let remainder = (bits_needed_per_pixel % color_type.channel_count()) as usize;
    for i in 0..remainder {
        data_bits_per_channel[i] += 1;
    }

    while data_bits_per_channel.len() < color_type.channel_count() as usize {
        data_bits_per_channel.insert(0, 0) // Right-Pad with empty data
    }

    // Finally, build the u64
    let bits_per_channel =
        (color_type.bits_per_pixel() / color_type.channel_count() as u16) as usize;
    let bytes_per_channel = (color_type.bytes_per_pixel() / color_type.channel_count()) as usize;

    let mut return_vec: Vec<u8> = Vec::new();

    for bits_for_current_channel in data_bits_per_channel {
        let mut vec_for_channel = vec![0u8; bytes_per_channel as usize];

        let clear_bits_count = (bits_per_channel - bits_for_current_channel) as usize;

        for i in clear_bits_count..bits_per_channel {
            vec_for_channel[i / 8] |= 0b1u8 << 7 >> i % 8;
        }

        return_vec.append(&mut vec_for_channel);
    }

    let mut return_data: u64 = 0;

    for i in 0..return_vec.len() {
        return_data |= (return_vec[i] as u64) << 64 - 8 >> i * 8;
    }

    return_data
}

pub(crate) fn generate_v1_header(
    pixel_count: u64,
    data_len_bytes: u64,
    color_type: ColorType,
) -> Result<VersionedHeader, String> {
    // start_offset + data_len + worst case data_mask (4B) + CRC32
    // Header is only using 1 bit per pixel.
    let v1_header_len = (size_of::<u64>() * 2 + 4 + size_of::<u32>()) as u64;
    let available_pixels = pixel_count - v1_header_len;

    // How many bits would we need to be able to encode the entire payload
    let bits_needed_per_pixel = (1 + (data_len_bytes * 8 / available_pixels)) as u8;
    let available_space_bytes = color_type.bytes_per_pixel() as u64 * available_pixels;

    if bits_needed_per_pixel as u16 > color_type.bits_per_pixel() {
        return Err(format!("Cannot encode data. Would need {}bytes, but can only encode {}bytes in the given picture. (delta: {})", data_len_bytes, available_space_bytes, data_len_bytes-available_space_bytes));
    }

    let pixels_needed_to_store_message = (data_len_bytes * 8) / bits_needed_per_pixel as u64 + 1;

    let offset = v1_header_len
        + thread_rng().gen_range(0..(available_pixels - pixels_needed_to_store_message));

    let header = VersionedHeader::V1 {
        stuffing_opts: V1DataStuffingOptions::None {
            start_offset: offset,
        },
        data_mask: calculate_bit_mask(bits_needed_per_pixel, color_type),
        data_len: data_len_bytes,
    };

    Ok(header)
}

#[cfg(test)]
mod tests {

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn calculate_bit_mask_rgb8() {
        // 3 Channels (RGB), 8 bits each. Should be 0b0000_1111__0000_1111__0000_1111__0000...0000
        let response = calculate_bit_mask(12, ColorType::Rgb8);
        assert_eq!(
            format!("{:#01x}", response),
            format!("{:#01x}", 0x0F_0F_0F_00__00_00_00_00u64)
        )
    }

    #[test]
    fn calculate_bit_mask_rgba8() {
        // 4 Channels (RGBA), 8 bits each. Should be 0b0000_1111__0000_1111__0000_1111__0000_1111_0...0000
        let response = calculate_bit_mask(16, ColorType::Rgba8);
        assert_eq!(
            format!("{:#01x}", response),
            format!("{:#01x}", 0x0F_0F_0F_0F__00_00_00_00u64)
        )
    }

    #[test]
    fn calculate_partial_bit_mask_rgba8() {
        let response = calculate_bit_mask(5, ColorType::Rgba8);
        assert_eq!(
            format!("{:#01x}", response),
            format!("{:#01x}", 0x03_01_01_01__00_00_00_00u64)
        )
    }

    fn util_count_bits(input: u64) -> usize {
        let mut counter = 0 as usize;

        for i in 0..64 {
            if input & 1u64 << i != 0 {
                counter += 1;
            }
        }
        counter
    }

    #[test]
    fn generate_v1_header_test() {
        let result = generate_v1_header(600, 100, ColorType::Rgb8).unwrap();

        match result {
            VersionedHeader::V1 {
                stuffing_opts,
                data_mask,
                data_len,
            } => {
                assert_eq!(data_len, 100);
                match stuffing_opts {
                    V1DataStuffingOptions::None { start_offset } => {
                        let bits_per_pixel = util_count_bits(data_mask);
                        assert_eq!(bits_per_pixel, 2);

                        let used_pixels_data = (data_len * 8) / bits_per_pixel as u64;

                        assert_eq!(used_pixels_data, 400);
                        assert!(start_offset + used_pixels_data < 600);
                    }
                }
            }
        }
    }

    #[test]
    fn encode_and_decode_v1_header() {
        let header = VersionedHeader::V1 {
            stuffing_opts: V1DataStuffingOptions::None {
                start_offset: 0x1234,
            },
            data_mask: 0xABCD,
            data_len: 0x98761234,
        };

        let as_raw_header: HeaderRaw = header.try_into().unwrap();

        let as_binary_data = bincode::encode_to_vec(as_raw_header, config::standard()).unwrap();

        let (as_raw_header_from_binary_data, _): (HeaderRaw, _) =
            bincode::decode_from_slice(&as_binary_data, config::standard()).unwrap();

        let header_from_raw = VersionedHeader::try_from(as_raw_header_from_binary_data).unwrap();

        assert_eq!(header, header_from_raw);
    }
}
