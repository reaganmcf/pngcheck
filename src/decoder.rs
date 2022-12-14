use crate::bit_depth::BitDepth;
use crate::buffer::Buffer;
use crate::chunk::{BackgroundData, Chunk, ChunkData, ChunkType, TransparencyData};
use crate::color_type::ColorType;
use crate::error::DecodeError;
use crate::interlace_method::InterlaceMethod;

pub struct Decoder {
    buffer: Buffer,
    chunks: Vec<Chunk>,
}

const PNG_SIGNATURE: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10];
impl Decoder {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            buffer: Buffer::new(bytes),
            chunks: Vec::new(),
        }
    }

    // TODO we should actually be returning some sort of like data structure
    // representing all that was decoded
    pub fn decode(&mut self) -> Result<(), DecodeError> {
        self.read_signature()?;
        self.read_chunk()?;

        // TODO: check that the first chunk is IHDR

        // TODO probably dont force unwrap?
        while self.chunks.last().unwrap().ty != ChunkType::IEND {
            self.read_chunk()?;
        }

        for chunk in self.chunks.iter() {
            println!("{:#?}", chunk);
        }

        Ok(())
    }

    fn get_color_type(&self) -> ColorType {
        let ihdr = self
            .chunks
            .first()
            .expect("the ihdr chunk should have already been decoded");

        match &ihdr.data {
            ChunkData::IHDR { color_type, .. } => *color_type,
            _ => panic!("the first chunk wasn't an ihdr"),
        }
    }

    fn read_signature(&mut self) -> Result<(), DecodeError> {
        let items = self.buffer.read_n(8)?;

        if items == PNG_SIGNATURE {
            return Ok(());
        }

        Err(DecodeError::MissingSignature)
    }

    //https://www.w3.org/TR/2003/REC-PNG-20031110/#table51
    fn read_chunk(&mut self) -> Result<(), DecodeError> {
        let length = self.buffer.read_u32()?;
        let ty: ChunkType = self.buffer.read_n(4)?.try_into()?;

        let data = match ty {
            ChunkType::IHDR => self.read_ihdr_chunk_data(length)?,
            ChunkType::IDAT => self.read_idat_chunk_data(length)?,
            ChunkType::IEND => ChunkData::IEND,
            ChunkType::gAMA => self.read_gama_chunk_data(length)?,
            ChunkType::PLTE => self.read_plte_chunk_data(length)?,
            ChunkType::bKGD => self.read_bkgd_chunk_data(length)?,
            ChunkType::tRNS => self.read_trns_chunk_data(length)?
        };

        let crc = self.buffer.read_u32()?;

        self.chunks.push(Chunk {
            length,
            ty,
            data,
            crc,
        });

        Ok(())
    }

    // https://www.w3.org/TR/2003/REC-PNG-20031110/#11IHDR
    fn read_ihdr_chunk_data(&mut self, length: u32) -> Result<ChunkData, DecodeError> {
        if length != 13 {
            return Err(DecodeError::InvalidIHDRLength);
        }

        let width = self.buffer.read_u32()?;
        let height = self.buffer.read_u32()?;
        let bit_depth: BitDepth = self.buffer.read_u8()?.try_into()?;
        let color_type: ColorType = self.buffer.read_u8()?.try_into()?;

        // TODO: Add proper support for compression_method field
        self.buffer.read_u8()?;

        // TODO: Add proper support for filter_method field
        self.buffer.read_u8()?;

        let interlace_method: InterlaceMethod = self.buffer.read_u8()?.try_into()?;

        println!("- read ihdr chunk data");
        Ok(ChunkData::IHDR {
            width,
            height,
            bit_depth,
            color_type,
            compression_method: 0,
            filter_method: 0,
            interlace_method,
        })
    }

    fn read_idat_chunk_data(&mut self, length: u32) -> Result<ChunkData, DecodeError> {
        let length: usize = length.try_into().unwrap();
        let bytes = Vec::from(self.buffer.read_n(length)?);

        Ok(ChunkData::IDAT(bytes))
    }

    fn read_gama_chunk_data(&mut self, _length: u32) -> Result<ChunkData, DecodeError> {
        // TODO: check length is 4

        // 11.3.3.2:
        //  The value is encoded as a four-byte PNG unsigned integer, representing gamma times 100000
        let image_gamma: f64 = f64::from(self.buffer.read_u32()?) / f64::from(100000);

        Ok(ChunkData::gAMA { image_gamma })
    }

    fn read_plte_chunk_data(&mut self, length: u32) -> Result<ChunkData, DecodeError> {
        let length: usize = length.try_into().unwrap();
        let mut entries = Vec::with_capacity(length / 3);

        for _ in 0..(length / 3) {
            let r = self.buffer.read_u8()?;
            let g = self.buffer.read_u8()?;
            let b = self.buffer.read_u8()?;

            entries.push((r, g, b));
        }

        Ok(ChunkData::PLTE(entries))
    }

    fn read_bkgd_chunk_data(&mut self, _length: u32) -> Result<ChunkData, DecodeError> {
        let inner = match self.get_color_type() {
            ColorType::_0 | ColorType::_4 => {
                let grayscale = self.buffer.read_u16()?;
                BackgroundData::Grayscale(grayscale)
            }
            ColorType::_2 | ColorType::_6 => {
                let red = self.buffer.read_u16()?;
                let green = self.buffer.read_u16()?;
                let blue = self.buffer.read_u16()?;

                BackgroundData::RGB((red, green, blue))
            }
            ColorType::_3 => {
                let palette_index = self.buffer.read_u8()?;
                BackgroundData::PaletteIndex(palette_index)
            }
        };

        Ok(ChunkData::bKGD(inner))
    }

    fn read_trns_chunk_data(&mut self, length: u32) -> Result<ChunkData, DecodeError> {
        let length: usize = length.try_into().unwrap();
        let inner = match self.get_color_type() {
            ColorType::_0 => {
                let graysample = self.buffer.read_u16()?;
                Ok(TransparencyData::Graysample(graysample))
            }
            ColorType::_2 => {
                let red = self.buffer.read_u16()?;
                let green = self.buffer.read_u16()?;
                let blue = self.buffer.read_u16()?;

                Ok(TransparencyData::RGB((red, green, blue)))
            }
            ColorType::_3 => {
                // TODO - check that there arent more entries than palette entries

                let mut indices = Vec::with_capacity(length);
                for _ in 0..length {
                    let index = self.buffer.read_u8()?;
                    indices.push(index);
                }

                Ok(TransparencyData::PaletteIndices(indices))
            }
            ColorType::_4 | ColorType::_6 => Err(DecodeError::UnexpectedtRNSChunk),
        }?;
        

        Ok(ChunkData::tRNS(inner))
    }
}
