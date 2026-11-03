//! Renders a frame after N scripted tics and writes it as a PPM image.
//!
//!   cargo run --example snapshot -- <tics> <actors> <out.ppm>

use engine::game::scripted_input;
use engine::{FB_BYTES, Game, H, PALETTE, W, render};
use std::io::Write;

/// Turns the camera toward the closest living creature (preview only; the
/// float math here never touches the deterministic core).
fn aim_at_nearest(game: &mut Game) {
    let (px, py) = (game.player.x as f64, game.player.y as f64);
    let nearest = game.actors[..game.actor_count]
        .iter()
        .filter(|a| a.state != engine::game::ActorState::Dead)
        .min_by_key(|a| ((a.x as f64 - px).hypot(a.y as f64 - py)) as i64);
    if let Some(a) = nearest {
        let turn = (a.y as f64 - py).atan2(a.x as f64 - px) / std::f64::consts::TAU;
        game.player.angle = ((turn * 1024.0).round() as i32).rem_euclid(1024) as u32;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tics: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let actors: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(16);
    let out = args.get(3).cloned().unwrap_or_else(|| "frame.ppm".into());

    let mut game = Game::new(1, actors);
    for t in 0..tics {
        game.tick(scripted_input(t));
    }
    if args.get(4).map(String::as_str) == Some("aim") {
        aim_at_nearest(&mut game);
    }
    let mut fb = [0u8; FB_BYTES];
    render::render(&game, &mut fb);

    let mut file = std::fs::File::create(&out).expect("create output");
    write!(file, "P6\n{W} {H}\n255\n").unwrap();
    for y in 0..H {
        for x in 0..W {
            file.write_all(&PALETTE[render::pixel(&fb, x, y) as usize]).unwrap();
        }
    }
    println!(
        "tic {} hp {} kills {} angle {} -> {out}",
        game.tic, game.player.hp, game.kills, game.player.angle
    );
}
