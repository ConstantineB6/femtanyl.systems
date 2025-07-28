use shared::Packet;
use std::collections::VecDeque;
use std::{cell::RefCell, collections::HashMap, f64::consts::PI, rc::Rc};
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{
    AudioContext, CanvasRenderingContext2d, GainNode, HtmlCanvasElement, OscillatorNode,
    PointerEvent, WebSocket, js_sys::Math,
};
use web_sys::{BiquadFilterNode, BiquadFilterType, MessageEvent, OscillatorType};

// Constants - Movement
const SPEED_FACTOR: f64 = 0.01;
const STOP_RADIUS: f64 = 0.0;
const TRAIL_LIFE: f64 = 1_000.0;

// Constants - Audio
const MAX_VOLUME: f64 = 2.0;

#[derive(Clone)]
struct Peer {
    x: f64,
    y: f64,
    color: String,
}

#[derive(Clone)]
struct TrailPoint {
    x: f64,
    y: f64,
    t: f64,
}

#[wasm_bindgen(start)]
pub fn run() -> Result<(), JsValue> {
    // Prepare window and canvas
    let win = web_sys::window().unwrap();
    let dpr = win.device_pixel_ratio(); // include device pixel ratio for higher visual quality

    let canvas: HtmlCanvasElement = win
        .document()
        .unwrap()
        .get_element_by_id("wb")
        .unwrap()
        .dyn_into()?;

    // Resize canvas to fit screen (high resolution)
    let w_css = win.inner_width()?.as_f64().unwrap();
    let h_css = win.inner_height()?.as_f64().unwrap();

    canvas.set_width((w_css * dpr) as u32);
    canvas.set_height((h_css * dpr) as u32);

    // keep the on-screen box in CSS pixels
    canvas
        .style()
        .set_property("width", &format!("{w_css}px"))?;
    canvas
        .style()
        .set_property("height", &format!("{h_css}px"))?;

    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.unwrap().dyn_into()?;
    ctx.scale(dpr, dpr)?;

    // ─── Audio set-up ───
    let audio_ctx = Rc::new(AudioContext::new()?);
    let oscillator: OscillatorNode = audio_ctx.create_oscillator()?;

    // pick a “pleasant” wave-form once per run
    let waves = [
        OscillatorType::Sine,
        OscillatorType::Sawtooth,
        OscillatorType::Square,
    ];
    let wf_idx = (Math::random() * waves.len() as f64).floor() as usize;
    oscillator.set_type(waves[wf_idx]);

    oscillator.frequency().set_value(200.0);
    let gain: GainNode = audio_ctx.create_gain()?;
    gain.gain().set_value(0.0);

    let filter: BiquadFilterNode = audio_ctx.create_biquad_filter()?; // ← new
    filter.set_type(BiquadFilterType::Lowpass);
    filter.frequency().set_value(1000.0);
    filter.q().set_value(0.7);

    // osc -> gain -> filter -> speakers
    oscillator.connect_with_audio_node(&gain)?;
    gain.connect_with_audio_node(&filter)?;
    filter.connect_with_audio_node(&audio_ctx.destination())?;

    oscillator.start()?;

    // shared among events
    let oscillator = Rc::new(oscillator);
    let gain = Rc::new(gain);
    let ctx = Rc::new(ctx);

    let target = Rc::new(RefCell::new((w_css * 0.5, h_css * 0.5)));
    let pos = Rc::new(RefCell::new((w_css * 0.5, h_css * 0.5)));

    // last timestamp - used to calc dt for speed
    let last_ts = Rc::new(RefCell::new(f64::NAN));

    // generate user color (always some kind of pastel)
    let hue = (Math::random() * 360.0).round(); // 0-360°
    let color = format!("hsl({hue}, 70%, 70%)");
    let color = Rc::new(color);

    // WebSocket setup (production url: wss://femtanyl-systems.fly.dev/ws)
    let ws = WebSocket::new("wss://femtanyl-systems.fly.dev/ws")?;
    let ws = Rc::new(ws);
    let peers_t = Rc::new(RefCell::new(HashMap::<String, Peer>::new()));
    let peers_p = Rc::new(RefCell::new(HashMap::<String, Peer>::new()));
    let trails = Rc::new(RefCell::new(HashMap::<String, VecDeque<TrailPoint>>::new()));
    let my_trail = Rc::new(RefCell::new(VecDeque::<TrailPoint>::new()));
    let my_id = Rc::new(RefCell::new(None::<String>));

    // Prevent flashbangs
    clear_canvas(&ctx, w_css, h_css);

    // onmessage -> update peers target
    {
        let peers_t = peers_t.clone();
        let my_id = my_id.clone();
        let my_color = color.clone();
        let cb = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Some(txt) = e.data().as_string() {
                if let Ok(pkt) = serde_json::from_str::<Packet>(&txt) {
                    // save my id
                    if my_id.borrow().is_none() && pkt.color == *my_color {
                        *my_id.borrow_mut() = Some(pkt.id.clone());
                    }

                    // store target for other players
                    if Some(&pkt.id) != my_id.borrow().as_ref() {
                        peers_t.borrow_mut().insert(
                            pkt.id.clone(),
                            Peer {
                                x: pkt.x as f64 * w_css,
                                y: pkt.y as f64 * h_css,
                                color: pkt.color,
                            },
                        );
                    }
                }
            }
        });
        ws.set_onmessage(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // onpointermove -> update target
    {
        let ws = ws.clone();
        let col = color.clone();
        let target = target.clone();
        let w_css = w_css;
        let h_css = h_css;

        let closure = Closure::<dyn FnMut(_)>::new(move |e: PointerEvent| {
            let x = e.offset_x() as f64;
            let y = e.offset_y() as f64;
            *target.borrow_mut() = (x, y);

            let pkt = Packet {
                id: String::new(), // server fills in
                color: col.clone().to_string(),
                x: (x / w_css) as f32,
                y: (y / h_css) as f32,
                extra: HashMap::new(),
            };
            let _ = ws.send_with_str(&serde_json::to_string(&pkt).unwrap());
        });
        canvas.add_event_listener_with_callback("pointermove", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // onanimationframe -> animate movement
    {
        let ctx = ctx.clone();
        let pos = pos.clone();
        let tgt = target.clone();
        let osc = oscillator.clone();
        let gn = gain.clone();
        let ts0 = last_ts.clone();
        let win = web_sys::window().unwrap();
        let win_cb = win.clone();

        let peers_t = peers_t.clone();
        let peers_p = peers_p.clone();

        let f = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
        let g = f.clone();

        *g.borrow_mut() = Some(Closure::wrap(Box::new(move |time: f64| {
            // clear canvas
            clear_canvas(&ctx, w_css, h_css);

            let dt = if ts0.borrow().is_nan() {
                0.0
            } else {
                time - *ts0.borrow()
            };
            *ts0.borrow_mut() = time;

            let now = time;

            // ─── animate peers movement ───
            for (id, tgt) in peers_t.borrow().iter() {
                let mut peers_p_ref = peers_p.borrow_mut();
                let current = peers_p_ref.entry(id.clone()).or_insert(tgt.clone());

                // interpolate
                let dx = tgt.x - current.x;
                let dy = tgt.y - current.y;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > STOP_RADIUS && dt > 0.0 {
                    let speed = dist * SPEED_FACTOR;
                    let step = (speed * dt).min(dist);
                    current.x += dx / dist * step;
                    current.y += dy / dist * step;
                }

                // draw
                ctx.begin_path();
                ctx.set_fill_style_str(&current.color);
                ctx.arc(current.x, current.y, 20., 0., 2. * PI).unwrap();
                ctx.fill();

                // sound from speed
                let speed = (current.x - tgt.x).abs() + (current.y - tgt.y).abs();
                osc.frequency().set_value((200.0 + speed * 800.0) as f32);
                gn.gain()
                    .set_value((speed * 2.0).min(0.1).clamp(0.0, MAX_VOLUME) as f32);

                {
                    let mut tr = trails.borrow_mut();
                    let q = tr.entry(id.clone()).or_default();
                    q.push_back(TrailPoint {
                        x: current.x,
                        y: current.y,
                        t: now,
                    });

                    // drop expired points
                    while q.front().map_or(false, |p| now - p.t > TRAIL_LIFE) {
                        q.pop_front();
                    }
                }

                // 2. draw trail (old → new, fading)
                if let Some(q) = trails.borrow().get(id) {
                    for p in q {
                        let age = now - p.t;
                        let alpha = 1.0 - age / TRAIL_LIFE; // 1 → 0
                        ctx.set_global_alpha(alpha);
                        ctx.begin_path();
                        ctx.set_fill_style_str(&current.color);
                        ctx.arc(p.x, p.y, 20.0 * alpha, 0.0, 2.0 * PI).unwrap();
                        ctx.fill();
                    }
                    ctx.set_global_alpha(1.0); // reset!
                }
            }

            // ─── self movement ───

            // pos
            let (tx, ty) = *tgt.borrow();
            let (mut x, mut y) = *pos.borrow_mut();

            let dx = tx - x;
            let dy = ty - y;
            let dist = (dx * dx + dy * dy).sqrt();

            let mut speed = 0.0;
            if dist > STOP_RADIUS && dt > 0.0 {
                speed = dist * SPEED_FACTOR;
                let step = (speed * dt).min(dist);
                let nx = dx / dist;
                let ny = dy / dist;
                x += nx * step;
                y += ny * step;
                *pos.borrow_mut() = (x, y);
            }

            // record my own trail
            let mut q = my_trail.borrow_mut();
            q.push_back(TrailPoint { x, y, t: now });
            while q.front().map_or(false, |p| now - p.t > TRAIL_LIFE) {
                q.pop_front();
            }

            // draw my trail
            for p in q.iter() {
                let age = now - p.t;
                let alpha = 1.0 - age / TRAIL_LIFE;
                ctx.set_global_alpha(alpha);
                ctx.begin_path();
                ctx.set_fill_style_str(&color);
                ctx.arc(p.x, p.y, 20.0 * alpha, 0.0, 2.0 * PI).unwrap();
                ctx.fill();
            }
            ctx.set_global_alpha(1.0);

            // draw
            ctx.begin_path();
            ctx.set_fill_style_str(&color);
            ctx.arc(x, y, 20., 0., 2. * PI).unwrap();
            ctx.fill();

            // sound from speed
            osc.frequency().set_value((200.0 + speed * 800.0) as f32);
            gn.gain()
                .set_value((speed * 2.0).min(0.1).clamp(0.0, MAX_VOLUME) as f32);

            // schedule next frame
            win_cb
                .request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref())
                .unwrap();
        }) as Box<dyn FnMut(f64)>));

        win.request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;
    }

    return Ok(());
}

fn clear_canvas(ctx: &CanvasRenderingContext2d, win_width: f64, win_height: f64) {
    ctx.set_fill_style_str("#121212");
    ctx.fill_rect(0., 0., win_width, win_height);
}
