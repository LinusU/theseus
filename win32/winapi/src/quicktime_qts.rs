use std::sync::atomic::{AtomicU32, Ordering};

use runtime::{Cont, Context, Memory};

static CURRENT_DATA_HANDLE: AtomicU32 = AtomicU32::new(0);
static CURRENT_DATA_SIZE: AtomicU32 = AtomicU32::new(0);
static CURRENT_GWORLD: AtomicU32 = AtomicU32::new(0);
static CURRENT_IMAGE_TYPE: AtomicU32 = AtomicU32::new(0);
static LAST_HANDLE: AtomicU32 = AtomicU32::new(0);
static LAST_HANDLE_SIZE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug)]
struct QuickDrawRect {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
}

impl QuickDrawRect {
    fn read(memory: &Memory, addr: u32) -> Self {
        Self {
            top: memory.read::<i16>(addr) as i32,
            left: memory.read::<i16>(addr + 2) as i32,
            bottom: memory.read::<i16>(addr + 4) as i32,
            right: memory.read::<i16>(addr + 6) as i32,
        }
    }

    fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Clone, Copy, Debug)]
struct QuickDrawPixMap {
    pixels: u32,
    row_bytes: u32,
    bounds: QuickDrawRect,
    depth: u16,
}

impl QuickDrawPixMap {
    fn read(memory: &Memory, addr: u32) -> Self {
        // CopyBits accepts either a BitMap/PixMap or a GrafPort/CGrafPort.
        // A color port is identified by the two high portVersion bits and
        // reaches its PixMap through the portPixMap handle.
        let port_version = memory.read::<u16>(addr + 4);
        let addr = if port_version & 0xc000 == 0xc000 {
            let pixmap_handle = memory.read::<u32>(addr);
            memory.read::<u32>(pixmap_handle)
        } else {
            addr
        };
        let row_bytes = memory.read::<u16>(addr + 4);
        Self {
            pixels: memory.read::<u32>(addr),
            row_bytes: (row_bytes & 0x3fff) as u32,
            bounds: QuickDrawRect::read(memory, addr + 6),
            // The high bits of rowBytes distinguish a color PixMap from the
            // original one-bit BitMap. Deimos uses color PixMaps here.
            depth: if row_bytes & 0x8000 != 0 {
                memory.read::<u16>(addr + 32)
            } else {
                1
            },
        }
    }

    fn pixel_addr(self, x: i32, y: i32) -> u32 {
        let x = (x - self.bounds.left) as u32;
        let y = (y - self.bounds.top) as u32;
        self.pixels + y * self.row_bytes + x * u32::from(self.depth).div_ceil(8)
    }
}

/// QuickDraw's CopyBits, sufficient for the color pixmaps Deimos uses to
/// compose its frame. Copy through a temporary buffer because sprite copies
/// can overlap within one GWorld.
fn copy_bits(memory: &mut Memory, src_bits: u32, dst_bits: u32, src_rect: u32, dst_rect: u32) {
    let src = QuickDrawPixMap::read(memory, src_bits);
    let dst = QuickDrawPixMap::read(memory, dst_bits);
    let src_rect = QuickDrawRect::read(memory, src_rect);
    let dst_rect = QuickDrawRect::read(memory, dst_rect);

    if src.depth != dst.depth || !matches!(src.depth, 8 | 16 | 32) {
        log::warn!(
            "unsupported QuickDraw CopyBits format {} -> {} bits",
            src.depth,
            dst.depth
        );
        return;
    }
    let src_width = src_rect.width();
    let src_height = src_rect.height();
    let dst_width = dst_rect.width();
    let dst_height = dst_rect.height();
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return;
    }

    let bytes_per_pixel = usize::from(src.depth.div_ceil(8));
    let mut copied = Vec::with_capacity((dst_width * dst_height) as usize);
    for dst_y in dst_rect.top..dst_rect.bottom {
        for dst_x in dst_rect.left..dst_rect.right {
            if !dst.bounds.contains(dst_x, dst_y) {
                continue;
            }
            let src_x = src_rect.left + (dst_x - dst_rect.left) * src_width / dst_width;
            let src_y = src_rect.top + (dst_y - dst_rect.top) * src_height / dst_height;
            if !src.bounds.contains(src_x, src_y) {
                continue;
            }
            let src_addr = src.pixel_addr(src_x, src_y);
            let mut pixel = [0_u8; 4];
            pixel[..bytes_per_pixel].copy_from_slice(&memory[src_addr..][..bytes_per_pixel]);
            copied.push((dst.pixel_addr(dst_x, dst_y), pixel));
        }
    }
    for (dst_addr, pixel) in copied {
        memory[dst_addr..][..bytes_per_pixel].copy_from_slice(&pixel[..bytes_per_pixel]);
    }
}

#[derive(Debug)]
struct IndexedImage {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    palette: Vec<u8>,
}

fn decode_indexed_gif(encoded: &[u8]) -> Result<IndexedImage, String> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    let mut decoder = options
        .read_info(std::io::Cursor::new(encoded))
        .map_err(|error| error.to_string())?;
    let frame = decoder
        .read_next_frame()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "GIF contains no image frame".to_owned())?
        .clone();
    let palette = frame
        .palette
        .clone()
        .or_else(|| decoder.global_palette().map(<[u8]>::to_vec))
        .ok_or_else(|| "GIF contains no color table".to_owned())?;

    Ok(IndexedImage {
        left: u32::from(frame.left),
        top: u32::from(frame.top),
        width: u32::from(frame.width),
        height: u32::from(frame.height),
        pixels: frame.buffer.into_owned(),
        palette,
    })
}

fn set_color_table(ctx: &mut Context, pixmap: u32, palette: &[u8]) {
    let color_table_handle = ctx.memory.read::<u32>(pixmap + 42);
    if color_table_handle == 0 {
        return;
    }
    let color_table = ctx.memory.read::<u32>(color_table_handle);
    if color_table == 0 {
        return;
    }

    let color_count = (palette.len() / 3).min(256);
    ctx.memory
        .write::<u16>(color_table + 6, color_count.saturating_sub(1) as u16);
    for index in 0..256_u32 {
        let entry = color_table + 8 + index * 8;
        let rgb_offset = index as usize * 3;
        let rgb = palette.get(rgb_offset..rgb_offset + 3).unwrap_or(&[0; 3]);
        ctx.memory.write::<u16>(entry, index as u16);
        ctx.memory.write::<u16>(entry + 2, u16::from(rgb[0]) * 257);
        ctx.memory.write::<u16>(entry + 4, u16::from(rgb[1]) * 257);
        ctx.memory.write::<u16>(entry + 6, u16::from(rgb[2]) * 257);
    }
}

fn rgb555(red: u8, green: u8, blue: u8) -> u16 {
    ((u16::from(red) >> 3) << 10) | ((u16::from(green) >> 3) << 5) | (u16::from(blue) >> 3)
}

fn decode_current_image(ctx: &mut Context) {
    let data_handle = CURRENT_DATA_HANDLE.load(Ordering::Relaxed);
    let data_size = CURRENT_DATA_SIZE.load(Ordering::Relaxed);
    let gworld = CURRENT_GWORLD.load(Ordering::Relaxed);
    if data_handle == 0 || data_size == 0 || gworld == 0 {
        log::warn!(
            "QuickTime draw missing data ({data_handle:#x}, {data_size}) or GWorld ({gworld:#x})"
        );
        return;
    }

    let data = ctx.memory.read::<u32>(data_handle);
    let encoded = ctx.memory[data..][..data_size as usize].to_vec();
    let pixmap_handle = ctx.memory.read::<u32>(gworld);
    let pixmap = ctx.memory.read::<u32>(pixmap_handle);
    let pixels = ctx.memory.read::<u32>(pixmap);
    let row_bytes = (ctx.memory.read::<u16>(pixmap + 4) & 0x3fff) as u32;
    let top = ctx.memory.read::<u16>(pixmap + 6) as i16 as i32;
    let left = ctx.memory.read::<u16>(pixmap + 8) as i16 as i32;
    let bottom = ctx.memory.read::<u16>(pixmap + 10) as i16 as i32;
    let right = ctx.memory.read::<u16>(pixmap + 12) as i16 as i32;
    let depth = ctx.memory.read::<u16>(pixmap + 32);
    let width = (right - left).max(0) as u32;
    let height = (bottom - top).max(0) as u32;

    match CURRENT_IMAGE_TYPE.load(Ordering::Relaxed) {
        // Deimos first imports each grayscale alpha GIF into an 8-bit GWorld,
        // then applies its gamma table directly to those bytes. QuickTime
        // converts the GIF palette entries to grayscale intensities during
        // that import; retaining the source palette indices would turn common
        // white-at-index-zero masks black and make their green matte opaque.
        0x4749_4620 => {
            let image = match decode_indexed_gif(&encoded) {
                Ok(image) => image,
                Err(error) => {
                    log::warn!("QuickTime GIF decode failed: {error}");
                    return;
                }
            };
            if depth == 8 {
                set_color_table(ctx, pixmap, &image.palette);
            } else if depth != 16 {
                log::warn!("unsupported QuickTime GIF GWorld depth {depth}");
                return;
            }

            for source_y in 0..image.height {
                let dest_y = image.top + source_y;
                if dest_y >= height {
                    continue;
                }
                for source_x in 0..image.width {
                    let dest_x = image.left + source_x;
                    if dest_x >= width {
                        continue;
                    }
                    let index = image.pixels[(source_y * image.width + source_x) as usize];
                    let pixel = pixels + dest_y * row_bytes;
                    let offset = usize::from(index) * 3;
                    let rgb = image.palette.get(offset..offset + 3).unwrap_or(&[0; 3]);
                    if depth == 8 {
                        let luminance = (u32::from(rgb[0]) * 77
                            + u32::from(rgb[1]) * 150
                            + u32::from(rgb[2]) * 29
                            + 128)
                            >> 8;
                        ctx.memory.write::<u8>(pixel + dest_x, luminance as u8);
                    } else {
                        ctx.memory.write::<u16>(
                            pixel + dest_x * 2,
                            rgb555(rgb[0], rgb[1], rgb[2]).swap_bytes(),
                        );
                    }
                }
            }
        }
        0x5447_4120 => {
            if depth != 16 {
                log::warn!("unsupported QuickTime TGA GWorld depth {depth}");
                return;
            }
            let image = match image::load_from_memory_with_format(&encoded, image::ImageFormat::Tga)
            {
                Ok(image) => image.to_rgba8(),
                Err(error) => {
                    log::warn!("QuickTime TGA decode failed: {error}");
                    return;
                }
            };
            let copy_width = width.min(image.width());
            let copy_height = height.min(image.height());
            for y in 0..copy_height {
                for x in 0..copy_width {
                    let rgba = image.get_pixel(x, y).0;
                    let value = if rgba[3] < 128 {
                        0
                    } else {
                        rgb555(rgba[0], rgba[1], rgba[2])
                    };
                    ctx.memory
                        .write::<u16>(pixels + y * row_bytes + x * 2, value.swap_bytes());
                }
            }
        }
        image_type => log::warn!("unsupported QuickTime image type {image_type:#010x}"),
    }
}

fn create_color_table(ctx: &mut Context) -> u32 {
    const COLOR_TABLE_SIZE: u32 = 8 + 256 * 8;

    let kernel32 = crate::kernel32::lock();
    let table = kernel32
        .process_heap
        .alloc(&mut ctx.memory, COLOR_TABLE_SIZE);
    let handle = kernel32.process_heap.alloc(&mut ctx.memory, 4);
    drop(kernel32);

    ctx.memory[table..][..COLOR_TABLE_SIZE as usize].fill(0);
    ctx.memory.write::<u16>(table + 6, 255);
    for index in 0..=255_u32 {
        let entry = table + 8 + index * 8;
        ctx.memory.write::<u16>(entry, index as u16);
    }
    ctx.memory.write::<u32>(handle, table);
    handle
}

fn dispatch(ctx: &mut Context, name: &str) -> Cont {
    let return_addr = ctx.pop32();
    let selector = ctx.cpu.regs.eax;
    let args: Vec<u32> = (0..8)
        .map(|i| ctx.memory.read::<u32>(ctx.cpu.regs.esp + i * 4))
        .collect();
    log::trace!("{name} selector={selector:#08x} at {return_addr:#010x}, args={args:08x?}");

    let result = match selector {
        // NewHandle(Size)
        0x15_0008 => {
            let kernel32 = crate::kernel32::lock();
            let data = kernel32.process_heap.alloc(&mut ctx.memory, args[0]);
            let handle = kernel32.process_heap.alloc(&mut ctx.memory, 4);
            drop(kernel32);

            ctx.memory[data..][..args[0] as usize].fill(0);
            ctx.memory.write::<u32>(handle, data);
            LAST_HANDLE.store(handle, Ordering::Relaxed);
            LAST_HANDLE_SIZE.store(args[0], Ordering::Relaxed);
            handle
        }
        // HLock(Handle)
        0x15_0019 => 0,
        // DisposeHandle(Handle)
        0x15_0032 => {
            let handle = args[0];
            if handle != 0 {
                let data = ctx.memory.read::<u32>(handle);
                let kernel32 = crate::kernel32::lock();
                if data != 0 {
                    kernel32.process_heap.free(&mut ctx.memory, data);
                }
                kernel32.process_heap.free(&mut ctx.memory, handle);
            }
            0
        }
        // OpenADefaultComponent(ComponentType, ComponentSubType, ComponentInstance*)
        0x01_002e => {
            CURRENT_IMAGE_TYPE.store(args[1], Ordering::Relaxed);
            ctx.memory.write::<u32>(args[2], 1);
            0
        }
        // CallComponent(ComponentInstance, ComponentParameters*)
        0x01_0000 => {
            let parameters = args[1];
            let what = ctx.memory.read::<u16>(parameters + 2);
            match what {
                // GraphicsImportSetDataHandle
                5 => {
                    let handle = ctx.memory.read::<u32>(parameters + 4);
                    CURRENT_DATA_HANDLE.store(handle, Ordering::Relaxed);
                    let size = if handle == LAST_HANDLE.load(Ordering::Relaxed) {
                        LAST_HANDLE_SIZE.load(Ordering::Relaxed)
                    } else {
                        0
                    };
                    CURRENT_DATA_SIZE.store(size, Ordering::Relaxed);
                }
                // GraphicsImportGetImageDescription
                7 => {
                    let output = ctx.memory.read::<u32>(parameters + 4);
                    let data_handle = CURRENT_DATA_HANDLE.load(Ordering::Relaxed);
                    let data = ctx.memory.read::<u32>(data_handle);
                    let image_type = CURRENT_IMAGE_TYPE.load(Ordering::Relaxed);

                    let (mut width, mut height, depth) = match image_type {
                        // 'GIF '
                        0x4749_4620 => (
                            ctx.memory.read::<u16>(data + 6),
                            ctx.memory.read::<u16>(data + 8),
                            8,
                        ),
                        // 'TGA '
                        0x5447_4120 => (
                            ctx.memory.read::<u16>(data + 12),
                            ctx.memory.read::<u16>(data + 14),
                            ctx.memory.read::<u8>(data + 16) as u16,
                        ),
                        _ => (320, 240, 16),
                    };
                    if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
                        width = 320;
                        height = 240;
                    }

                    let kernel32 = crate::kernel32::lock();
                    let description = kernel32.process_heap.alloc(&mut ctx.memory, 128);
                    let description_handle = kernel32.process_heap.alloc(&mut ctx.memory, 4);
                    drop(kernel32);

                    ctx.memory[description..][..128].fill(0);
                    ctx.memory.write::<u32>(description, 128);
                    ctx.memory.write::<u32>(description + 4, image_type);
                    ctx.memory.write::<u16>(description + 0x20, width);
                    ctx.memory.write::<u16>(description + 0x22, height);
                    ctx.memory.write::<u16>(description + 0x52, depth);
                    ctx.memory.write::<u32>(description_handle, description);
                    ctx.memory.write::<u32>(output, description_handle);
                }
                // GraphicsImportDraw
                15 => decode_current_image(ctx),
                // GraphicsImportSetGWorld
                16 => {
                    CURRENT_GWORLD.store(ctx.memory.read::<u32>(parameters + 4), Ordering::Relaxed);
                }
                _ => log::warn!("unhandled QuickTime component selector {what}"),
            }
            0
        }
        // SetRect(Rect*, left, top, right, bottom)
        0x1e_0025 => {
            let rect = args[0];
            ctx.memory.write::<u16>(rect, args[2] as u16);
            ctx.memory.write::<u16>(rect + 2, args[1] as u16);
            ctx.memory.write::<u16>(rect + 4, args[4] as u16);
            ctx.memory.write::<u16>(rect + 6, args[3] as u16);
            0
        }
        // NewGWorld(GWorldPtr*, depth, Rect*, CTabHandle, GDHandle, flags)
        0x1c_0001 => {
            let bounds = args[2];
            let top = ctx.memory.read::<u16>(bounds) as i16 as i32;
            let left = ctx.memory.read::<u16>(bounds + 2) as i16 as i32;
            let bottom = ctx.memory.read::<u16>(bounds + 4) as i16 as i32;
            let right = ctx.memory.read::<u16>(bounds + 6) as i16 as i32;
            let width = (right - left).max(0) as u32;
            let height = (bottom - top).max(0) as u32;
            let depth = args[1] as u16;
            let unaligned_row_bytes = width * (depth as u32).div_ceil(8);
            let row_bytes = (unaligned_row_bytes + 3) & !3;

            let kernel32 = crate::kernel32::lock();
            let pixels = kernel32
                .process_heap
                .alloc(&mut ctx.memory, row_bytes * height);
            let pixmap = kernel32.process_heap.alloc(&mut ctx.memory, 64);
            let pixmap_handle = kernel32.process_heap.alloc(&mut ctx.memory, 4);
            let gworld = kernel32.process_heap.alloc(&mut ctx.memory, 128);
            drop(kernel32);
            let color_table = if depth == 8 {
                create_color_table(ctx)
            } else {
                0
            };

            ctx.memory[pixels..][..(row_bytes * height) as usize].fill(0);
            ctx.memory[pixmap..][..64].fill(0);
            ctx.memory[gworld..][..128].fill(0);
            ctx.memory.write::<u32>(pixmap, pixels);
            ctx.memory
                .write::<u16>(pixmap + 4, 0x8000 | row_bytes as u16);
            ctx.memory.write::<u16>(pixmap + 6, top as u16);
            ctx.memory.write::<u16>(pixmap + 8, left as u16);
            ctx.memory.write::<u16>(pixmap + 10, bottom as u16);
            ctx.memory.write::<u16>(pixmap + 12, right as u16);
            ctx.memory.write::<u16>(pixmap + 32, depth);
            if depth == 8 {
                ctx.memory.write::<u16>(pixmap + 30, 0); // Indexed
                ctx.memory.write::<u16>(pixmap + 34, 1);
                ctx.memory.write::<u16>(pixmap + 36, 8);
            } else {
                ctx.memory.write::<u16>(pixmap + 30, 0x10); // RGBDirect
                ctx.memory.write::<u16>(pixmap + 34, 3);
                ctx.memory.write::<u16>(pixmap + 36, 5);
            }
            ctx.memory.write::<u32>(pixmap + 42, color_table);
            ctx.memory.write::<u32>(pixmap_handle, pixmap);
            ctx.memory.write::<u32>(gworld, pixmap_handle);
            // The GWorld begins with a CGrafPort. Its color-port marker tells
            // QuickDraw callers that the first field is a PixMapHandle rather
            // than a raw BitMap base address.
            ctx.memory.write::<u16>(gworld + 4, 0xc000);
            ctx.memory.write::<u32>(args[0], gworld);
            0
        }
        // LockPixels(PixMapHandle)
        0x1c_0002 => 1,
        // DisposeGWorld(GWorldPtr)
        0x1c_0005 => {
            let gworld = args[0];
            if gworld != 0 {
                let pixmap_handle = ctx.memory.read::<u32>(gworld);
                let pixmap = ctx.memory.read::<u32>(pixmap_handle);
                let pixels = ctx.memory.read::<u32>(pixmap);
                let color_table_handle = ctx.memory.read::<u32>(pixmap + 42);
                let color_table = if color_table_handle != 0 {
                    ctx.memory.read::<u32>(color_table_handle)
                } else {
                    0
                };

                let kernel32 = crate::kernel32::lock();
                for allocation in [
                    pixels,
                    pixmap,
                    pixmap_handle,
                    gworld,
                    color_table,
                    color_table_handle,
                ] {
                    if allocation != 0 {
                        kernel32.process_heap.free(&mut ctx.memory, allocation);
                    }
                }
            }
            0
        }
        // SetGWorld(GWorldPtr, GDHandle)
        0x1c_0007 => 0,
        // GetPixBaseAddr(PixMapHandle)
        0x1c_0010 => {
            let pixmap = ctx.memory.read::<u32>(args[0]);
            ctx.memory.read::<u32>(pixmap)
        }
        // GetGWorldPixMap(GWorldPtr)
        0x1c_0018 => ctx.memory.read::<u32>(args[0]),
        // SetPort(CGrafPtr), used with the embedded port in the GWorld.
        0x1e_002e => 0,
        // CopyBits(srcBits, dstBits, srcRect, dstRect, mode, maskRgn)
        0x1e_0058 => {
            if args[4] != 0 || args[5] != 0 {
                log::warn!(
                    "unsupported QuickDraw CopyBits mode {} or mask {:#x}",
                    args[4],
                    args[5]
                );
            } else {
                copy_bits(&mut ctx.memory, args[0], args[1], args[2], args[3]);
            }
            0
        }
        _ => 0,
    };

    ctx.cpu.regs.eax = result;
    ctx.indirect(return_addr)
}

#[allow(non_snake_case)]
pub fn theQuickTimeDispatcher_stdcall(ctx: &mut Context) -> Cont {
    dispatch(ctx, "theQuickTimeDispatcher")
}

#[allow(non_snake_case)]
pub fn _CallComponent_stdcall(ctx: &mut Context) -> Cont {
    dispatch(ctx, "_CallComponent")
}

#[allow(non_snake_case)]
pub fn _CallComponentFunctionWithStorage_stdcall(ctx: &mut Context) -> Cont {
    dispatch(ctx, "_CallComponentFunctionWithStorage")
}
