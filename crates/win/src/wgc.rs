//! Frames of the game window through Windows Graphics Capture, so they keep arriving while the game is covered
//! by other windows (the Store Permit watcher works in the background).
//!
//! This is the API behind OBS's and Discord's "window capture": the system compositor hands over the pixels the
//! game already rendered. The game process is not opened and nothing is injected into it. The yellow capture
//! border is turned off where Windows allows it (Windows 11). A minimized window delivers no frames.

use maplelens_core::image::RgbImage;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND, POINT, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{Interface, Result};

pub struct WindowCapture {
    hwnd: HWND,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt_device: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
}

// SAFETY: the D3D device is free-threaded, the frame pool was created free-threaded, and a WindowCapture is only
// ever used from the one watcher thread that owns it.
unsafe impl Send for WindowCapture {}

impl WindowCapture {
    /// True where Windows offers window capture at all (Windows 10 1903+, not some server or remote sessions).
    pub fn supported() -> bool {
        GraphicsCaptureSession::IsSupported().unwrap_or(false)
    }

    pub fn new(hwnd: HWND) -> Result<Self> {
        if !Self::supported() {
            return Err(windows::core::Error::new(windows::core::HRESULT(-1), "window capture is not supported here"));
        }
        // WinRT must be initialised on the calling thread (the shop thread's OCR engine does it too; repeating it is
        // harmless). Without it the capture APIs run on an uninitialised thread, which is undefined behaviour.
        // SAFETY: per-thread WinRT initialisation, as in `ocr::WindowsOcr::new`.
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let (device, context) = d3d_device()?;
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: a valid DXGI device; the WinRT wrapper keeps its own reference.
        let winrt_device: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        // SAFETY: `hwnd` is a top-level window handle; a stale one only makes this fail.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd)? };
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        // Windows 11 lets an app drop the yellow border once it has asked; older systems keep it.
        if let Ok(request) = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless) {
            let _ = request.join();
        }
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok(Self { hwnd, device, context, winrt_device, pool, session, size, staging: None })
    }

    /// The newest frame since the last call, cropped to the client area; None if nothing new arrived (the window is
    /// minimized, or the game drew nothing since).
    pub fn latest(&mut self) -> Result<Option<RgbImage>> {
        let mut newest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            newest = Some(frame);
        }
        let Some(frame) = newest else { return Ok(None) };
        let content = frame.ContentSize()?;
        if (content.Width, content.Height) != (self.size.Width, self.size.Height) {
            // The window was resized: the next frames come at the new size.
            self.size = content;
            self.pool.Recreate(&self.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 1, content)?;
            return Ok(None);
        }
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        // SAFETY: the surface of a frame from our own pool is a D3D11 texture.
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: out-parameter is a valid local.
        unsafe { texture.GetDesc(&mut desc) };
        let staging = self.staging(&desc)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: copying between two textures of the same size and format, then reading the CPU-side copy while
        // it is mapped; `pData` covers `RowPitch * Height` bytes.
        let image = unsafe {
            self.context.CopyResource(&staging, &texture);
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let (w, h) = (desc.Width as usize, desc.Height as usize);
            let rows = std::slice::from_raw_parts(mapped.pData as *const u8, mapped.RowPitch as usize * h);
            let image = self.client_crop(rows, mapped.RowPitch as usize, w, h);
            self.context.Unmap(&staging, 0);
            image
        };
        Ok(Some(image))
    }

    fn staging(&mut self, desc: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D> {
        if let Some((texture, w, h)) = &self.staging
            && (*w, *h) == (desc.Width, desc.Height)
        {
            return Ok(texture.clone());
        }
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..*desc
        };
        let mut texture = None;
        // SAFETY: a valid description; the texture is kept alive in `self.staging`.
        unsafe { self.device.CreateTexture2D(&staging_desc, None, Some(&mut texture))? };
        let texture = texture.ok_or_else(windows::core::Error::empty)?;
        self.staging = Some((texture.clone(), desc.Width, desc.Height));
        Ok(texture)
    }

    /// The frame is the window's visible frame (title bar and borders included); cut out the client area.
    fn client_crop(&self, rows: &[u8], pitch: usize, width: usize, height: usize) -> RgbImage {
        let (mut frame, mut client, mut origin) = (RECT::default(), RECT::default(), POINT::default());
        // SAFETY: plain queries with valid out-parameters.
        let found = unsafe {
            DwmGetWindowAttribute(
                self.hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&mut frame as *mut RECT).cast(),
                size_of::<RECT>() as u32,
            )
            .is_ok()
                && GetClientRect(self.hwnd, &mut client).is_ok()
                && ClientToScreen(self.hwnd, &mut origin).as_bool()
        };
        let (x0, y0, w, h) = if found {
            let x0 = (origin.x - frame.left).clamp(0, width as i32) as usize;
            let y0 = (origin.y - frame.top).clamp(0, height as i32) as usize;
            (x0, y0, (client.right as usize).min(width - x0), (client.bottom as usize).min(height - y0))
        } else {
            (0, 0, width, height)
        };
        let mut data = Vec::with_capacity(w * h * 3);
        for y in y0..y0 + h {
            for px in rows[y * pitch + x0 * 4..][..w * 4].as_chunks::<4>().0 {
                data.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
        RgbImage { width: w, height: h, data }
    }
}

impl Drop for WindowCapture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

fn d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    // The GPU first; without one (a virtual machine, a remote session) Windows' software rasteriser.
    let mut last = windows::core::Error::empty();
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let (mut device, mut context) = (None, None);
        // SAFETY: out-parameters are valid locals; BGRA support is what the capture surfaces need.
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        match (created, device, context) {
            (Ok(()), Some(device), Some(context)) => return Ok((device, context)),
            (Err(error), ..) => last = error,
            _ => {}
        }
    }
    Err(last)
}
