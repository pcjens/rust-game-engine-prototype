mod descriptors {
    use core::ops::Range;

    #[derive(Debug)]
    pub struct ChunkDescriptor {
        /// The range of bytes in the chunk data portion of the database this
        /// texture chunk can be loaded from.
        pub source_bytes: Range<u64>,
    }

    #[derive(Debug)]
    pub struct TextureChunkDescriptor {
        /// The width of the texture the chunk contains.
        pub region_width: u16,
        /// The height of the texture the chunk contains.
        pub region_height: u16,
        /// The range of bytes in the chunk data portion of the database this
        /// texture chunk can be loaded from.
        pub source_bytes: Range<u64>,
    }
}

mod loaded {
    use core::fmt::Debug;

    use platform_abstraction_layer::TextureRef;

    use crate::{
        resources::{
            asset_index::AssetIndex, CHUNK_SIZE, TEXTURE_CHUNK_DIMENSIONS, TEXTURE_CHUNK_FORMAT,
        },
        LinearAllocator,
    };

    /// Loaded memory for a single regular chunk. Contains [`CHUNK_SIZE`] bytes.
    #[repr(C, align(64))]
    pub struct LoadedChunk(pub [u8; CHUNK_SIZE as usize]);

    impl Debug for LoadedChunk {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "LoadedChunk({} KiB of data)", CHUNK_SIZE / 1024)
        }
    }

    /// Loaded (video) memory for a single texture chunk. Contains a reference to a
    /// loaded texture, ready for drawing, with the size and format
    /// [`TEXTURE_CHUNK_DIMENSIONS`] and [`TEXTURE_CHUNK_FORMAT`].
    pub struct LoadedTextureChunk(pub TextureRef);

    impl Debug for LoadedTextureChunk {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let (w, h) = TEXTURE_CHUNK_DIMENSIONS;
            let bpp = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
            let kibs = w as usize * h as usize * bpp / 1024;
            write!(f, "LoadedTextureChunk({w}x{h} texture, {kibs} KiB)")
        }
    }

    /// Holds chunks currently loaded in-memory.
    pub struct ChunkStorage<'eng> {
        /// Contains loaded in-memory chunks. Matches the layout of
        /// [`AssetIndex::chunks`].
        pub chunks: SparselyPopulatedArray<'eng, LoadedChunk>,
        /// Contains loaded in-memory texture chunks. Matches the layout of
        /// [`AssetIndex::texture_chunks`].
        pub texture_chunks: SparselyPopulatedArray<'eng, LoadedTextureChunk>,
    }

    impl<'eng> ChunkStorage<'eng> {
        /// Creates a new [`ChunkStorage`] with the given maximum capacities for
        /// regular in-memory chunks and texture memory chunks, returning None
        /// if the required backing memory couldn't be allocated from
        /// `allocator`.
        ///
        /// NOTE: The specific [`AssetIndex`] passed to this function should be
        /// passed to subsequent uses of this [`ChunkStorage`] as well. While it
        /// isn't unsafe to use other asset indexes with this ChunkStorage,
        /// unless they have indentical chunk descriptors, it will lead to
        /// panics due to unmet expectations.
        pub fn new(
            allocator: &'eng LinearAllocator,
            asset_index: &AssetIndex,
            max_chunks: u32,
            max_texture_chunks: u32,
        ) -> Option<ChunkStorage<'eng>> {
            Some(ChunkStorage {
                chunks: SparselyPopulatedArray::new(
                    allocator,
                    asset_index.chunks.len() as u32,
                    max_chunks,
                )?,
                texture_chunks: SparselyPopulatedArray::new(
                    allocator,
                    asset_index.texture_chunks.len() as u32,
                    max_texture_chunks,
                )?,
            })
        }
    }

    use sparsely_populated_array::SparselyPopulatedArray;
    mod sparsely_populated_array {
        use core::num::NonZeroU32;

        use bytemuck::Zeroable;

        use crate::{FixedVec, LinearAllocator};

        pub struct SparselyPopulatedArray<'eng, T> {
            /// The indirection array: contains indexes to `resident_data` for
            /// the array elements which are currently loaded.
            resident_indices: FixedVec<'eng, OptionalU32>,
            /// Reusable indexes to `resident_data`.
            free_indices: FixedVec<'eng, u32>,
            /// The currently loaded instances of T.
            resident_data: FixedVec<'eng, T>,
        }

        impl<T> SparselyPopulatedArray<'_, T> {
            pub fn new<'a>(
                allocator: &'a LinearAllocator,
                array_len: u32,
                resident_len: u32,
            ) -> Option<SparselyPopulatedArray<'a, T>> {
                let mut resident_indices = FixedVec::new(allocator, array_len as usize)?;
                resident_indices.fill_with_zeroes();

                Some(SparselyPopulatedArray {
                    resident_indices,
                    free_indices: FixedVec::new(allocator, resident_len as usize)?,
                    resident_data: FixedVec::new(allocator, resident_len as usize)?,
                })
            }

            /// Removes the value from the index, freeing space for another
            /// value to be inserted anywhere.
            pub fn unload(&mut self, index: u32) {
                if let Some(resident_index) = self
                    .resident_indices
                    .get_mut(index as usize)
                    .and_then(OptionalU32::take)
                {
                    self.free_indices.push(resident_index).unwrap();
                }
            }

            /// Allocates space for the index if there's space, returning a
            /// mutable borrow to fill it with. If reuse is not possible and a
            /// new T needs to be created, `init_fn` is used. `init_fn` can fail
            /// by returning None, in which case nothing else happens and None
            /// is returned.
            pub fn insert(
                &mut self,
                index: u32,
                init_fn: impl FnOnce() -> Option<T>,
            ) -> Option<&mut T> {
                let now_resident_index =
                    if let Some(reused_resident_index) = self.free_indices.pop() {
                        reused_resident_index
                    } else {
                        let new_data = init_fn()?;
                        let new_resident_index = self.resident_data.len() as u32;
                        self.resident_data.push(new_data).ok()?;
                        new_resident_index
                    };
                self.resident_indices[index as usize].set(now_resident_index);
                Some(&mut self.resident_data[now_resident_index as usize])
            }

            /// Returns the value at the index if it's been inserted.
            pub fn get(&self, index: u32) -> Option<&T> {
                let maybe_resident_index = self.resident_indices.get(index as usize).unwrap();
                let resident_index = maybe_resident_index.get()?;
                Some(&self.resident_data[resident_index as usize])
            }
        }

        /// `Option<u32>` but Zeroable and u32-sized.
        ///
        /// But can't contain 0xFFFFFFFF.
        #[derive(Zeroable, Clone, Copy)]
        struct OptionalU32 {
            // TODO: Instead of just index | "is it resident", maybe these index
            // slots should contain bits for "keep this around forever" and
            // "this has been requested"? 29 bits for loaded chunks should still
            // leave plenty of capacity.
            index_plus_one: Option<NonZeroU32>,
        }

        impl OptionalU32 {
            pub fn set(&mut self, index: u32) {
                self.index_plus_one = Some(NonZeroU32::new(index + 1).unwrap());
            }

            pub fn get(self) -> Option<u32> {
                self.index_plus_one
                    .map(|index_plus_one| index_plus_one.get() - 1)
            }

            pub fn take(&mut self) -> Option<u32> {
                let result = self.get();
                self.index_plus_one = None;
                result
            }
        }
    }
}

pub use descriptors::{ChunkDescriptor, TextureChunkDescriptor};
pub use loaded::{ChunkStorage, LoadedChunk, LoadedTextureChunk};
