use gabagool::{Instance, Module, RawValue, Store};
use softbuffer::Surface;
use std::num::NonZeroU32;
use std::process::Command;
use std::rc::Rc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

const CELL_PX: usize = 8;
const SNAPSHOT_FILE: &str = "game-of-life.gabagool";

// const PALETTE_NAMES: &[&str] = &["amber", "green", "blue", "pink", "white"];

fn call_i32(store: &mut Store, instance: Instance, name: &str) -> gabagool::Result<i32> {
    let result = store.invoke(instance, name, vec![])?;
    let v = result.into_completed()?[0].as_i32();
    Ok(v)
}

fn read_framebuf(store: &Store, ptr: usize, len: usize) -> &[u32] {
    let data = &store.memories[0].data;
    let bytes = &data[ptr..ptr + len * 4];
    unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const u32, len) }
}

fn save_snapshot(store: &Store, path: &str) {
    let bytes = store.to_bytes();
    if let Err(e) = std::fs::write(path, &bytes) {
        eprintln!("failed to write snapshot: {e}");
    } else {
        // println!("Snapshot saved to {path} ({} bytes)\n", bytes.len());
    }
}

fn fork_snapshot(store: &Store, parent_x: i32, parent_y: i32) {
    let bytes = store.to_bytes();

    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let path = format!("game-of-life-{id}.gabagool");
    if let Err(e) = std::fs::write(&path, &bytes) {
        eprintln!("fork snapshot error: {e}");
        return;
    }

    // println!("Forked from snapshot: {path}]\n");

    let exe = std::env::current_exe().expect("failed to get current exe");
    if let Err(e) = Command::new(exe)
        .arg("--restore")
        .arg(&path)
        .arg("--pos")
        .arg(format!("{},{}", parent_x + 400, parent_y + 400))
        .spawn()
    {
        eprintln!("failed to fork: {e}");
    }
}

struct App {
    store: Store,
    instance: Instance,
    framebuf_ptr: usize,
    win_size: usize,
    win_pos: Option<(i32, i32)>,
    paused: bool,
    palette: usize,
    cursor_pos: (f64, f64),
    mouse_down: bool,
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
}

impl App {
    fn place_cell(&mut self, px: f64, py: f64) {
        let gx = px as i32 / CELL_PX as i32;
        let gy = py as i32 / CELL_PX as i32;
        let _ = self.store.invoke(
            self.instance,
            "place_block",
            vec![RawValue::from(gx), RawValue::from(gy)],
        );
    }

    fn blit(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let framebuf = read_framebuf(
            &self.store,
            self.framebuf_ptr,
            self.win_size * self.win_size,
        );
        let mut buf = surface.buffer_mut().unwrap();
        buf.copy_from_slice(framebuf);
        buf.present().unwrap();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = winit::dpi::PhysicalSize::new(self.win_size as u32, self.win_size as u32);
        let mut attrs = Window::default_attributes()
            .with_title("Game of Life")
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size)
            .with_resizable(false);
        if let Some((x, y)) = self.win_pos {
            attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(x, y));
        }

        let window = Rc::new(event_loop.create_window(attrs).unwrap());

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let mut surface = Surface::new(&context, window.clone()).unwrap();
        surface
            .resize(
                NonZeroU32::new(self.win_size as u32).unwrap(),
                NonZeroU32::new(self.win_size as u32).unwrap(),
            )
            .unwrap();

        self.window = Some(window);
        self.surface = Some(surface);

        self.blit();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),
                        state: ElementState::Pressed,
                        repeat: false,
                        ..
                    },
                ..
            } => match key {
                KeyCode::Escape => event_loop.exit(),
                KeyCode::Space => {
                    self.paused = !self.paused;
                    // println!("{}", if self.paused { "Paused" } else { "Resumed" });
                }
                KeyCode::KeyS => {
                    save_snapshot(&self.store, SNAPSHOT_FILE);
                    if let Ok(idx) = call_i32(&mut self.store, self.instance, "cycle_palette") {
                        self.palette = idx as usize;
                    }
                    self.blit();
                }
                KeyCode::KeyF => {
                    if let Some(w) = &self.window {
                        let pos = w.outer_position().unwrap_or_default();
                        fork_snapshot(&self.store, pos.x, pos.y);
                    }
                }
                _ => {}
            },
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x, position.y);
                if self.mouse_down {
                    self.place_cell(position.x, position.y);
                }
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if button == MouseButton::Left {
                    self.mouse_down = state == ElementState::Pressed;
                    if self.mouse_down {
                        self.place_cell(self.cursor_pos.0, self.cursor_pos.1);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if !self.paused {
                    let _ = self.store.invoke(self.instance, "step", vec![]);

                    if let Ok(gen) = call_i32(&mut self.store, self.instance, "get_generation") {
                        if let Some(w) = &self.window {
                            w.set_title(&format!("gen: {gen}"));
                        }
                    }
                }

                self.blit();

                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let restore_path = args
        .iter()
        .position(|a| a == "--restore")
        .and_then(|i| args.get(i + 1).map(|s| s.as_str()));
    let win_pos: Option<(i32, i32)> = args.iter().position(|a| a == "--pos").and_then(|i| {
        let s = args.get(i + 1)?;
        let (x, y) = s.split_once(',')?;
        Some((x.parse().ok()?, y.parse().ok()?))
    });

    let (mut store, instance) = if let Some(path) = restore_path {
        let bytes = std::fs::read(path)?;
        let store = Store::from_bytes(&bytes);
        let instance = store.instance(0);
        (store, instance)
    } else {
        let wasm_bytes = include_bytes!("../wasm/game.wasm");
        let module = Module::new(wasm_bytes)?;
        let mut store = Store::new();
        let instance = store.instantiate(&module, vec![])?;
        store.invoke(instance, "init", vec![])?;
        (store, instance)
    };

    let grid_size = call_i32(&mut store, instance, "get_grid_size")? as usize;
    let win_size = grid_size * CELL_PX;
    let framebuf_ptr = call_i32(&mut store, instance, "get_framebuf_ptr")? as usize;

    store.invoke(instance, "render", vec![])?;

    let mut app = App {
        store,
        instance,
        framebuf_ptr,
        win_size,
        win_pos,
        paused: false,
        palette: 0,
        cursor_pos: (0.0, 0.0),
        mouse_down: false,
        window: None,
        surface: None,
    };

    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut app)?;

    Ok(())
}
