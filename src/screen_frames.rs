// Recording frames through DXGI Desktop Duplication: the GPU hands over the monitor's image
// only when it changes, and just the recorded region is copied back to system memory.
// GDI costs 29 ms a frame at 1080p on David's machine; this costs a few.

use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication,
    IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_ROTATION_IDENTITY;
use windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_ROTATION_UNSPECIFIED;

use crate::annotate::Rect;

pub struct ScreenFrames {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput1,
    duplication: Option<IDXGIOutputDuplication>,
    staging: ID3D11Texture2D,
    /// The recorded region relative to the monitor's own top-left corner.
    local: Rect,
    /// The same region on the virtual desktop, for placing the cursor.
    pub screen: Rect,
}

/// The monitor a point is on, with its rectangle on the virtual desktop.
fn output_at(x: i32, y: i32) -> windows::core::Result<Option<(IDXGIAdapter1, IDXGIOutput1, RECT)>> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                let desc = output.GetDesc()?;
                let r = desc.DesktopCoordinates;
                if x >= r.left && x < r.right && y >= r.top && y < r.bottom {
                    return Ok(Some((adapter, output.cast()?, r)));
                }
                o += 1;
            }
            a += 1;
        }
        Ok(None)
    }
}

/// Trim a region to the monitor it is recorded on, and to even width and height: H.264
/// stores colour at half resolution in both directions, so an odd size cannot be encoded.
pub fn fit_to_monitor(region: Rect, monitor: RECT) -> Option<Rect> {
    let x0 = region.0.max(monitor.left);
    let y0 = region.1.max(monitor.top);
    let x1 = (region.0 + region.2).min(monitor.right);
    let y1 = (region.1 + region.3).min(monitor.bottom);
    let (w, h) = ((x1 - x0) & !1, (y1 - y0) & !1);
    (w >= 16 && h >= 16).then_some((x0, y0, w, h))
}

impl ScreenFrames {
    /// Open duplication on the monitor holding the centre of `region` (virtual-desktop
    /// coordinates). The region is trimmed to that monitor: v1 records one monitor.
    pub fn open(region: Rect) -> Result<ScreenFrames, String> {
        unsafe { Self::open_inner(region) }.map_err(|e| format!("screen capture: {e}"))
    }

    unsafe fn open_inner(region: Rect) -> windows::core::Result<ScreenFrames> {
        let (cx, cy) = (region.0 + region.2 / 2, region.1 + region.3 / 2);
        let Some((adapter, output, monitor)) = output_at(cx, cy)? else {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_INVALIDARG,
                "the region is not on any monitor",
            ));
        };
        let Some(screen) = fit_to_monitor(region, monitor) else {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_INVALIDARG,
                "the region is too small to record",
            ));
        };

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        let device = device.expect("D3D11CreateDevice returned no device");
        let context = context.expect("D3D11CreateDevice returned no context");

        let duplication = output.DuplicateOutput(&device)?;
        let rotation = duplication.GetDesc().Rotation;
        if rotation != DXGI_MODE_ROTATION_IDENTITY && rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_NOTIMPL,
                "recording a rotated monitor is not supported yet",
            ));
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: screen.2 as u32,
            Height: screen.3 as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging))?;

        Ok(ScreenFrames {
            device,
            context,
            output,
            duplication: Some(duplication),
            staging: staging.expect("CreateTexture2D returned nothing"),
            local: (screen.0 - monitor.left, screen.1 - monitor.top, screen.2, screen.3),
            screen,
        })
    }

    pub fn width(&self) -> u32 {
        self.screen.2 as u32
    }

    pub fn height(&self) -> u32 {
        self.screen.3 as u32
    }

    /// Wait up to `timeout_ms` for the screen to change and copy the region if it did.
    /// Returns whether a new image arrived. A static screen simply times out, and the last
    /// copy stays valid — which is what a frame that did not change should show.
    pub fn wait_for_change(&mut self, timeout_ms: u32) -> Result<bool, String> {
        unsafe {
            if self.duplication.is_none() {
                // Lost earlier (a secure desktop, a mode change, a full-screen game).
                // Keep trying to get it back rather than ending the recording.
                match self.output.DuplicateOutput(&self.device) {
                    Ok(d) => self.duplication = Some(d),
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
                        return Ok(false);
                    }
                }
            }
            let duplication = self.duplication.as_ref().expect("checked above");
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            match duplication.AcquireNextFrame(timeout_ms, &mut info, &mut resource) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(false),
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                    self.duplication = None;
                    return Ok(false);
                }
                Err(e) => return Err(format!("reading the screen: {e}")),
            }
            let copied = resource
                .ok_or_else(|| "no frame".to_string())
                .and_then(|r| r.cast::<ID3D11Texture2D>().map_err(|e| e.to_string()))
                .map(|texture| {
                    let (x, y, w, h) = self.local;
                    let area = D3D11_BOX {
                        left: x as u32,
                        top: y as u32,
                        front: 0,
                        right: (x + w) as u32,
                        bottom: (y + h) as u32,
                        back: 1,
                    };
                    self.context.CopySubresourceRegion(
                        &self.staging,
                        0,
                        0,
                        0,
                        0,
                        &texture,
                        0,
                        Some(&area),
                    );
                });
            let _ = duplication.ReleaseFrame();
            copied.map(|_| true)
        }
    }

    /// The last region image as top-down BGRA into `dst`, rows `stride` bytes apart.
    pub fn read_into(&self, dst: &mut [u8], stride: usize) -> Result<(), String> {
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| format!("mapping the frame: {e}"))?;
            let row = self.width() as usize * 4;
            let src = mapped.pData as *const u8;
            for y in 0..self.height() as usize {
                std::ptr::copy_nonoverlapping(
                    src.add(y * mapped.RowPitch as usize),
                    dst.as_mut_ptr().add(y * stride),
                    row,
                );
            }
            self.context.Unmap(&self.staging, 0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    #[test]
    fn a_region_is_trimmed_to_its_monitor_and_to_even_sizes() {
        let m = monitor(0, 0, 2560, 1440);
        assert_eq!(fit_to_monitor((100, 100, 801, 601), m), Some((100, 100, 800, 600)));
        assert_eq!(fit_to_monitor((2400, 1300, 400, 400), m), Some((2400, 1300, 160, 140)));
    }

    #[test]
    fn a_monitor_left_of_the_primary_keeps_its_negative_coordinates() {
        let left = monitor(-1920, 182, 0, 1262);
        assert_eq!(fit_to_monitor((-1920, 182, 1920, 1080), left), Some((-1920, 182, 1920, 1080)));
    }

    #[test]
    fn a_sliver_is_not_worth_recording() {
        assert_eq!(fit_to_monitor((0, 0, 10, 500), monitor(0, 0, 1920, 1080)), None);
    }
}
