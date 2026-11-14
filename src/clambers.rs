//! Clambers: a small side-scrolling cave flyer hidden in Limoniter (type "clambers" into Find a command). Fly the
//! pod through four stages of terrain, keep its fuel up by hitting fuel canisters, shoot forward and drop bombs, and
//! reach the Engine at the end to knock out its core.
//!
//! The game is drawn on a 256×192 screen of 16 colours, a pixel at a time, and runs at 50 frames a second, as an
//! 8-bit machine would have it. Everything here is the game's own: the stages are made from a seeded random
//! generator, so each run is the same course. This module knows nothing of the window; `clambers_host` draws it.

pub const WIDTH: i32 = 256;
pub const HEIGHT: i32 = 192;
/// The top rows hold the score and fuel.
const HUD: i32 = 16;
/// The columns a stage lasts, at one column a frame.
const STAGE_LENGTH: i32 = 1600;
const STAGES: [&str; 4] = ["The Downs", "Sea Caves", "Crystal Shafts", "The Engine"];

/// The 16 colours, as RGB.
pub const PALETTE: [[u8; 3]; 16] = [
    [0x08, 0x08, 0x12], // 0 night
    [0x1e, 0x2b, 0x5a], // 1 deep blue
    [0x3a, 0x6f, 0xd8], // 2 blue
    [0x7f, 0xd4, 0xff], // 3 sky
    [0x1d, 0x5c, 0x2a], // 4 dark green
    [0x46, 0xb0, 0x4a], // 5 green
    [0xa6, 0xf0, 0x6a], // 6 lime
    [0x6b, 0x3e, 0x1f], // 7 brown
    [0xb8, 0x7a, 0x3c], // 8 sand
    [0xd8, 0x3a, 0x2e], // 9 red
    [0xff, 0x8c, 0x1a], // 10 orange
    [0xff, 0xe0, 0x4a], // 11 yellow
    [0xd0, 0x5a, 0xd8], // 12 violet
    [0x8a, 0x8f, 0xa0], // 13 grey
    [0xe8, 0xe8, 0xf0], // 14 white
    [0x00, 0x00, 0x00], // 15 black
];

/// What the player is pressing this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub fire: bool,
    pub bomb: bool,
    /// Pressed this frame (not held): to start, and to go on after losing a pod.
    pub start: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Title,
    Playing,
    /// The pod's been lost: a moment's pause, then on (or game over).
    Lost(u32),
    /// A stage cleared: its name shown before the next.
    Cleared(u32),
    Over(u32),
    /// The Engine's core is out.
    Won(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Rises from a vent in a wave.
    Spore,
    /// Waits on the ground, then launches straight up when the pod comes near.
    Dart,
    /// Swoops at the pod in caves.
    Bat,
    /// Hangs in the shafts and falls when the pod passes under.
    Crystal,
    /// Sits on the ground: hit it for fuel.
    Canister,
    /// The Engine's core: bomb it to win.
    Core,
}

#[derive(Clone, Debug)]
struct Thing {
    kind: Kind,
    /// Its place in the world: x in world columns, y in screen rows, both in 1/16ths of a pixel.
    x: i32,
    y: i32,
    vy: i32,
    age: u32,
    alive: bool,
    hits: u32,
}

#[derive(Clone, Copy, Debug)]
struct Shot {
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    bomb: bool,
}

#[derive(Clone, Copy, Debug)]
struct Spark {
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    life: u32,
    colour: u8,
}

/// A small, steady random generator, so a stage is always the same course.
#[derive(Clone, Debug)]
struct Random(u32);

impl Random {
    fn next(&mut self) -> u32 {
        // xorshift32.
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u32) -> i32 {
        (self.next() % n.max(1)) as i32
    }
}

/// A stage's ground and roof, column by column, and what's placed in it.
#[derive(Clone, Debug)]
struct Course {
    floor: Vec<i32>,
    roof: Vec<i32>,
    things: Vec<Thing>,
}

/// Makes a stage's terrain and puts things in it.
fn course(stage: usize) -> Course {
    let mut random = Random(0x9E37_79B9 ^ (stage as u32 + 1).wrapping_mul(0x85EB_CA6B));
    let length = (STAGE_LENGTH + WIDTH) as usize;
    let mut floor = vec![HEIGHT - 20; length];
    let mut roof = vec![HUD; length];
    let mut level = HEIGHT - 30;
    let mut top = HUD + 10;
    let mut slope = 0i32;
    for x in 0..length {
        // Every 16 columns the ground picks a new slope, kept in its stage's band.
        if x % 16 == 0 {
            slope = random.below(5) - 2;
        }
        let gentle = x < WIDTH as usize;
        match stage {
            0 => {
                level = (level + if gentle { 0 } else { slope }).clamp(HEIGHT - 70, HEIGHT - 14);
                top = HUD;
            }
            1 => {
                level = (level + slope).clamp(HEIGHT - 60, HEIGHT - 16);
                top = (top - slope / 2 + random.below(3) - 1).clamp(HUD + 4, HUD + 50);
            }
            2 => {
                // Shafts: walls that come and go, the gap moving up and down.
                let wave = ((x as f32 / 40.0).sin() * 30.0) as i32;
                level = (HEIGHT - 50 + wave + slope).clamp(HEIGHT - 90, HEIGHT - 16);
                top = (HUD + 40 + wave + random.below(3) - 1).clamp(HUD + 4, level - 52);
            }
            _ => {
                // The Engine: a steady tunnel of plates.
                let step = if (x / 32) % 2 == 0 { 6 } else { 0 };
                level = HEIGHT - 28 - step;
                top = HUD + 18 + step;
            }
        }
        floor[x] = level;
        roof[x] = top.min(level - 44);
    }
    let mut things = Vec::new();
    let mut place = |kind: Kind, x: i32, y: i32| things.push(Thing { kind, x: x * 16, y: y * 16, vy: 0, age: 0, alive: true, hits: 0 });
    let mut x = WIDTH + 40;
    while x < STAGE_LENGTH - 40 {
        let ground = floor[x as usize];
        let ceiling = roof[x as usize];
        // Fuel comes often enough to keep a careful pilot going.
        if random.below(5) == 0 {
            place(Kind::Canister, x, ground - 6);
        } else {
            match (stage, random.below(3)) {
                (0, 0) => place(Kind::Spore, x, ground - 4),
                (0 | 1, _) if random.below(2) == 0 => place(Kind::Dart, x, ground - 5),
                (1, _) => place(Kind::Bat, x, ceiling + 12),
                (2, 0) => place(Kind::Crystal, x, ceiling + 6),
                (2, _) => place(Kind::Spore, x, ground - 4),
                (3, 0) => place(Kind::Bat, x, ceiling + 14),
                _ => place(Kind::Dart, x, ground - 5),
            }
        }
        x += 28 + random.below(36);
    }
    if stage == STAGES.len() - 1 {
        place(Kind::Core, STAGE_LENGTH - 20, floor[(STAGE_LENGTH - 20) as usize] - 10);
    }
    Course { floor, roof, things }
}

/// A game of Clambers.
pub struct Game {
    pub phase: Phase,
    pub score: u32,
    pub best: u32,
    pub lives: u32,
    /// Fuel, out of 1000.
    pub fuel: i32,
    pub stage: usize,
    /// How far into the stage the screen's left edge is, in columns.
    pub scroll: i32,
    /// The pod's place on the screen, in 1/16ths of a pixel.
    pub x: i32,
    pub y: i32,
    course: Course,
    shots: Vec<Shot>,
    sparks: Vec<Spark>,
    fire_wait: u32,
    bomb_wait: u32,
    frame: u32,
    random: Random,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Game {
        let mut game = Game { phase: Phase::Title, score: 0, best: 0, lives: 3, fuel: 1000, stage: 0, scroll: 0, x: 0, y: 0, course: course(0), shots: Vec::new(), sparks: Vec::new(), fire_wait: 0, bomb_wait: 0, frame: 0, random: Random(12345) };
        game.place_pod();
        game
    }

    fn place_pod(&mut self) {
        self.x = 40 * 16;
        self.y = (HUD + 50) * 16;
        self.shots.clear();
    }

    fn start(&mut self) {
        self.score = 0;
        self.lives = 3;
        self.begin_stage(0);
        self.phase = Phase::Playing;
    }

    fn begin_stage(&mut self, stage: usize) {
        self.stage = stage;
        self.course = course(stage);
        self.scroll = 0;
        self.fuel = 1000;
        self.place_pod();
    }

    /// The ground's and roof's heights at a screen column.
    fn ground_at(&self, column: i32) -> (i32, i32) {
        let at = (self.scroll + column).clamp(0, self.course.floor.len() as i32 - 1) as usize;
        (self.course.roof[at], self.course.floor[at])
    }

    fn burst(&mut self, x: i32, y: i32, colour: u8, count: u32) {
        for _ in 0..count {
            let vx = self.random.below(64) - 32;
            let vy = self.random.below(64) - 40;
            self.sparks.push(Spark { x, y, vx, vy, life: 20 + self.random.below(20) as u32, colour });
        }
    }

    /// Runs one frame.
    pub fn tick(&mut self, controls: Controls) {
        self.frame = self.frame.wrapping_add(1);
        self.sparks.retain_mut(|s| {
            s.x += s.vx;
            s.y += s.vy;
            s.vy += 3;
            s.life = s.life.saturating_sub(1);
            s.life > 0
        });
        match self.phase {
            Phase::Title => {
                if controls.start {
                    self.start();
                }
            }
            Phase::Playing => self.play(controls),
            Phase::Lost(n) => {
                self.phase = if n == 0 {
                    if self.lives == 0 {
                        self.best = self.best.max(self.score);
                        Phase::Over(150)
                    } else {
                        // On from a little way back, with a full tank.
                        self.scroll = (self.scroll - 120).max(0);
                        self.fuel = 1000;
                        self.place_pod();
                        Phase::Playing
                    }
                } else {
                    Phase::Lost(n - 1)
                };
            }
            Phase::Cleared(n) => {
                self.phase = if n == 0 {
                    self.begin_stage(self.stage + 1);
                    Phase::Playing
                } else {
                    Phase::Cleared(n - 1)
                };
            }
            Phase::Over(n) | Phase::Won(n) => {
                let next = n.saturating_sub(1);
                if n == 0 && controls.start || next == 0 && controls.start {
                    self.phase = Phase::Title;
                } else if let Phase::Over(_) = self.phase {
                    self.phase = Phase::Over(next);
                } else {
                    self.phase = Phase::Won(next);
                }
            }
        }
    }

    fn play(&mut self, controls: Controls) {
        // The world moves on; the pod flies within the screen.
        if self.scroll < STAGE_LENGTH - WIDTH {
            self.scroll += 1;
        }
        let speed = 24;
        if controls.up {
            self.y -= speed;
        }
        if controls.down {
            self.y += speed;
        }
        if controls.left {
            self.x -= speed;
        }
        if controls.right {
            self.x += speed;
        }
        self.x = self.x.clamp(8 * 16, (WIDTH / 2) * 16);
        self.y = self.y.clamp((HUD + 4) * 16, (HEIGHT - 6) * 16);
        // Fuel burns steadily; flying hard burns more.
        self.fuel -= if controls.right { 2 } else { 1 };
        if self.fuel <= 0 {
            self.fuel = 0;
            // Out of fuel, the pod sinks.
            self.y += 20;
        }
        // Shots forward, bombs that fall and run on with the pod's speed.
        self.fire_wait = self.fire_wait.saturating_sub(1);
        self.bomb_wait = self.bomb_wait.saturating_sub(1);
        if controls.fire && self.fire_wait == 0 && self.shots.iter().filter(|s| !s.bomb).count() < 3 {
            self.shots.push(Shot { x: self.x + 10 * 16, y: self.y, vx: 96, vy: 0, bomb: false });
            self.fire_wait = 8;
        }
        if controls.bomb && self.bomb_wait == 0 && self.shots.iter().filter(|s| s.bomb).count() < 2 {
            self.shots.push(Shot { x: self.x, y: self.y + 4 * 16, vx: 20, vy: 8, bomb: true });
            self.bomb_wait = 14;
        }
        // Shots move, and stop at the ground or the screen's edge.
        let mut shots = std::mem::take(&mut self.shots);
        shots.retain_mut(|shot| {
            shot.x += shot.vx;
            shot.y += shot.vy;
            if shot.bomb {
                shot.vy = (shot.vy + 3).min(56);
            }
            let (roof, floor) = self.ground_at(shot.x / 16);
            let (px, py) = (shot.x / 16, shot.y / 16);
            if px >= WIDTH || py <= roof || py >= floor {
                if shot.bomb && py >= floor {
                    self.burst(shot.x, shot.y, 8, 6);
                }
                return false;
            }
            true
        });
        self.shots = shots;
        // Things on screen act, and are hit.
        let left = self.scroll * 16;
        let pod = (self.x + left, self.y);
        let mut things = std::mem::take(&mut self.course.things);
        let mut lost = false;
        for thing in things.iter_mut().filter(|t| t.alive) {
            let on_screen = thing.x >= left - 16 * 16 && thing.x <= left + (WIDTH + 16) * 16;
            if !on_screen {
                continue;
            }
            thing.age += 1;
            self.act(thing, pod);
            // Shots and bombs, in world terms.
            let (tx, ty) = (thing.x, thing.y);
            let reach = if thing.kind == Kind::Core { 12 * 16 } else { 6 * 16 };
            let mut hit = false;
            self.shots.retain(|shot| {
                let (sx, sy) = (shot.x + left, shot.y);
                let close = (sx - tx).abs() < reach && (sy - ty).abs() < reach;
                // Shots can't hurt the core: only bombs can.
                if close && !(thing.kind == Kind::Core && !shot.bomb) {
                    hit = true;
                    return false;
                }
                true
            });
            if hit {
                thing.hits += 1;
                let (needed, points, colour) = match thing.kind {
                    Kind::Canister => (1, 150, 6),
                    Kind::Spore => (1, 80, 12),
                    Kind::Dart => (1, if thing.vy != 0 { 100 } else { 50 }, 10),
                    Kind::Bat => (1, 120, 13),
                    Kind::Crystal => (2, 200, 3),
                    Kind::Core => (5, 2000, 11),
                };
                if thing.hits >= needed {
                    thing.alive = false;
                    self.score += points;
                    if thing.kind == Kind::Canister {
                        self.fuel = (self.fuel + 300).min(1000);
                    }
                    self.burst(thing.x - left, thing.y, colour, 14);
                    if thing.kind == Kind::Core {
                        self.burst(thing.x - left, thing.y, 9, 40);
                        self.best = self.best.max(self.score + self.lives * 1000);
                        self.score += self.lives * 1000;
                        self.phase = Phase::Won(250);
                    }
                }
            }
            // The pod against things: anything but a canister is fatal.
            if thing.alive && thing.kind != Kind::Canister && (thing.x - pod.0).abs() < 8 * 16 && (thing.y - pod.1).abs() < 6 * 16 {
                lost = true;
            }
        }
        self.course.things = things;
        // The pod against the ground.
        let (roof, floor) = self.ground_at(self.x / 16);
        let (roof_front, floor_front) = self.ground_at(self.x / 16 + 7);
        let py = self.y / 16;
        if py - 3 <= roof.max(roof_front) || py + 3 >= floor.min(floor_front) {
            lost = true;
        }
        if lost && matches!(self.phase, Phase::Playing) {
            self.lives = self.lives.saturating_sub(1);
            self.burst(self.x, self.y, 10, 30);
            self.phase = Phase::Lost(90);
            return;
        }
        // The end of a stage: on to the next (the Engine has to be won by its core).
        if self.scroll >= STAGE_LENGTH - WIDTH && self.stage + 1 < STAGES.len() && matches!(self.phase, Phase::Playing) && self.x / 16 > WIDTH / 2 - 8 {
            self.score += 500;
            self.phase = Phase::Cleared(120);
        } else if self.scroll >= STAGE_LENGTH - WIDTH && self.stage + 1 < STAGES.len() && matches!(self.phase, Phase::Playing) {
            // The ground's stopped: fly on to the right to leave the stage.
            self.x += 8;
        }
    }

    /// What a thing does each frame.
    fn act(&mut self, thing: &mut Thing, pod: (i32, i32)) {
        match thing.kind {
            Kind::Spore => {
                thing.vy = -10;
                thing.y += thing.vy;
                thing.x += ((thing.age as f32 / 8.0).sin() * 16.0) as i32;
                if thing.y < HUD * 16 {
                    thing.alive = false;
                }
            }
            Kind::Dart => {
                // Launches when the pod is near and in front of it.
                if thing.vy == 0 && thing.x - pod.0 < 50 * 16 && thing.x > pod.0 && self.random.below(30) == 0 {
                    thing.vy = -36;
                }
                thing.y += thing.vy;
                if thing.y < HUD * 16 {
                    thing.alive = false;
                }
            }
            Kind::Bat => {
                // Drifts back towards the pod's height, flapping.
                let toward = (pod.1 - thing.y).signum() * 10;
                thing.y += toward + if (thing.age / 6).is_multiple_of(2) { 6 } else { -6 };
                thing.x -= 12;
            }
            Kind::Crystal => {
                if thing.vy == 0 && (thing.x - pod.0).abs() < 20 * 16 {
                    thing.vy = 4;
                }
                if thing.vy != 0 {
                    thing.vy = (thing.vy + 2).min(48);
                    thing.y += thing.vy;
                }
                if thing.y > HEIGHT * 16 {
                    thing.alive = false;
                }
            }
            Kind::Canister | Kind::Core => {}
        }
    }

    /// The screen: one colour index a pixel, row by row.
    pub fn screen(&self) -> Vec<u8> {
        let mut screen = vec![0u8; (WIDTH * HEIGHT) as usize];
        let mut set = |x: i32, y: i32, c: u8| {
            if (0..WIDTH).contains(&x) && (0..HEIGHT).contains(&y) {
                screen[(y * WIDTH + x) as usize] = c;
            }
        };
        // Sky (or cave dark), ground and roof, stage by stage.
        let (sky, ground, edge, rock) = match self.stage {
            0 => (1, 4, 5, 1),
            1 => (0, 7, 8, 7),
            2 => (0, 1, 3, 1),
            _ => (0, 13, 14, 13),
        };
        for column in 0..WIDTH {
            let (roof, floor) = self.ground_at(column);
            for y in HUD..HEIGHT {
                let c = if y >= floor {
                    if y == floor || (y + column + self.scroll) % 9 == 0 { edge } else { ground }
                } else if y <= roof && self.stage > 0 {
                    if y == roof { edge } else { rock }
                } else if self.stage == 0 && (column * 7 + y * 13 + self.scroll / 2 * 7) % 211 == 0 {
                    14
                } else {
                    sky
                };
                set(column, y, c);
            }
        }
        // Things.
        let left = self.scroll * 16;
        for thing in self.course.things.iter().filter(|t| t.alive) {
            let (x, y) = ((thing.x - left) / 16, thing.y / 16);
            if !(-12..WIDTH + 12).contains(&x) {
                continue;
            }
            let (art, colour): (&[&str], u8) = match thing.kind {
                Kind::Canister => (&[" ### ", "#F#F#", "#####", "#FFF#", " ### "], 6),
                Kind::Spore => (&[" # ", "###", " # "], if (thing.age / 4) % 2 == 0 { 12 } else { 11 }),
                Kind::Dart => (&[" # ", " # ", "###", "# #"], 10),
                Kind::Bat => (if (thing.age / 6) % 2 == 0 { &["#   #", "## ##", " ### "] } else { &["     ", "#####", " # # "] }, 13),
                Kind::Crystal => (&[" # ", "###", "###", " # "], 3),
                Kind::Core => (&["   ###   ", " ####### ", "##     ##", "#  ###  #", "#  # #  #", "#  ###  #", "##     ##", " ####### ", "   ###   "], if (self.frame / 8).is_multiple_of(2) { 11 } else { 9 }),
            };
            draw_art(&mut set, x, y, art, colour);
        }
        // Shots and bombs.
        for shot in &self.shots {
            let (x, y) = (shot.x / 16, shot.y / 16);
            if shot.bomb {
                draw_art(&mut set, x, y, &["##", "##"], 11);
            } else {
                for dx in -2..3 {
                    set(x + dx, y, 14);
                }
            }
        }
        // The pod, unless it's just been lost.
        if matches!(self.phase, Phase::Playing | Phase::Cleared(_)) {
            let (x, y) = (self.x / 16, self.y / 16);
            draw_art(&mut set, x, y, &["  ##     ", " ######  ", "#########", " ###### #", "  ##     "], 14);
            set(x + 4, y, 2);
            // A flicker of exhaust.
            if self.frame % 4 < 2 {
                set(x - 5, y, 10);
                set(x - 6, y, 9);
            }
        }
        for spark in &self.sparks {
            set(spark.x / 16, spark.y / 16, spark.colour);
        }
        // The top: score, fuel and lives.
        for y in 0..HUD {
            for x in 0..WIDTH {
                set(x, y, 15);
            }
        }
        text(&mut set, 2, 2, &format!("{:06}", self.score), 14);
        text(&mut set, 2, 9, &format!("HI {:06}", self.best.max(self.score)), 13);
        text(&mut set, 92, 2, "FUEL", 11);
        let gauge = self.fuel * 80 / 1000;
        for x in 0..80 {
            for y in 3..7 {
                set(116 + x, y, if x < gauge { if gauge < 20 { 9 } else { 6 } } else { 1 });
            }
        }
        for life in 0..self.lives.min(5) {
            draw_art(&mut set, 204 + life as i32 * 10, 4, &[" ## ", "####", " ## "], 14);
        }
        text(&mut set, 92, 9, STAGES[self.stage.min(STAGES.len() - 1)], 3);
        // The words for each phase.
        let centre = |set: &mut dyn FnMut(i32, i32, u8), y: i32, words: &str, c: u8| text(set, (WIDTH - words.len() as i32 * 6) / 2, y, words, c);
        match self.phase {
            Phase::Title => {
                for y in HUD..HEIGHT {
                    for x in 0..WIDTH {
                        set(x, y, if (x + y) % 16 == 0 { 1 } else { 0 });
                    }
                }
                big_text(&mut set, (WIDTH - 8 * 12) / 2, 50, "CLAMBERS", 11);
                centre(&mut set, 90, "FLY THE POD THROUGH FOUR STAGES", 14);
                centre(&mut set, 100, "AND KNOCK OUT THE ENGINE CORE", 14);
                centre(&mut set, 120, "ARROWS OR WASD TO FLY", 3);
                centre(&mut set, 130, "SPACE TO FIRE - SHIFT TO BOMB", 3);
                centre(&mut set, 140, "HIT CANISTERS FOR FUEL", 6);
                if (self.frame / 25).is_multiple_of(2) {
                    centre(&mut set, 165, "PRESS SPACE TO START", 11);
                }
            }
            Phase::Lost(_) if self.lives > 0 => centre(&mut set, 90, "POD LOST", 9),
            Phase::Lost(_) => centre(&mut set, 90, "POD LOST", 9),
            Phase::Cleared(_) => {
                centre(&mut set, 80, "STAGE CLEAR", 6);
                if let Some(next) = STAGES.get(self.stage + 1) {
                    centre(&mut set, 95, &format!("NEXT: {}", next.to_ascii_uppercase()), 14);
                }
            }
            Phase::Over(_) => {
                centre(&mut set, 85, "GAME OVER", 9);
                centre(&mut set, 100, "PRESS SPACE", 13);
            }
            Phase::Won(_) => {
                centre(&mut set, 80, "THE ENGINE IS STILL", 11);
                centre(&mut set, 95, "WELL FLOWN, PILOT", 6);
                centre(&mut set, 110, "PRESS SPACE", 13);
            }
            Phase::Playing => {}
        }
        screen
    }
}

/// Draws a picture made of # marks (and F for a second colour) centred on a point.
fn draw_art(set: &mut dyn FnMut(i32, i32, u8), x: i32, y: i32, art: &[&str], colour: u8) {
    let (w, h) = (art.first().map_or(0, |r| r.len()) as i32, art.len() as i32);
    for (row, line) in art.iter().enumerate() {
        for (column, c) in line.chars().enumerate() {
            let (px, py) = (x - w / 2 + column as i32, y - h / 2 + row as i32);
            match c {
                '#' => set(px, py, colour),
                'F' => set(px, py, 14),
                _ => {}
            }
        }
    }
}

/// A 5×5 font of capitals, digits and a few marks, a row a byte (top bit is the left pixel of 5).
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x1F, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x1E, 0x11, 0x1E],
        'C' => [0x0F, 0x10, 0x10, 0x10, 0x0F],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x1E, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x1E, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x13, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x1F, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x1C, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x1E, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x1E, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x0E, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x0A, 0x04, 0x0A, 0x11],
        'Y' => [0x11, 0x0A, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x02, 0x04, 0x08, 0x1F],
        '0' => [0x0E, 0x13, 0x15, 0x19, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x0E],
        '2' => [0x1E, 0x01, 0x0E, 0x10, 0x1F],
        '3' => [0x1E, 0x01, 0x0E, 0x01, 0x1E],
        '4' => [0x12, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x1E],
        '6' => [0x0E, 0x10, 0x1E, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x04],
        '8' => [0x0E, 0x11, 0x0E, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x0F, 0x01, 0x0E],
        ':' => [0x00, 0x04, 0x00, 0x04, 0x00],
        '-' => [0x00, 0x00, 0x0E, 0x00, 0x00],
        ',' => [0x00, 0x00, 0x00, 0x04, 0x08],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x04],
        '!' => [0x04, 0x04, 0x04, 0x00, 0x04],
        _ => [0; 5],
    }
}

fn text(set: &mut dyn FnMut(i32, i32, u8), x: i32, y: i32, words: &str, colour: u8) {
    for (i, c) in words.chars().enumerate() {
        let rows = glyph(c);
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                if bits & (0x10 >> column) != 0 {
                    set(x + i as i32 * 6 + column, y + row as i32, colour);
                }
            }
        }
    }
}

/// Text twice the size, for the title.
fn big_text(set: &mut dyn FnMut(i32, i32, u8), x: i32, y: i32, words: &str, colour: u8) {
    for (i, c) in words.chars().enumerate() {
        let rows = glyph(c);
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                if bits & (0x10 >> column) != 0 {
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (px, py) = (x + i as i32 * 12 + column * 2 + dx, y + row as i32 * 2 + dy);
                        // A shadow below and right.
                        set(px + 1, py + 1, 9);
                        set(px, py, colour);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(start: bool) -> Controls {
        Controls { start, ..Default::default() }
    }

    #[test]
    fn a_game_starts_from_the_title_and_the_pod_flies() {
        let mut game = Game::new();
        assert_eq!(game.phase, Phase::Title);
        game.tick(press(true));
        assert_eq!(game.phase, Phase::Playing);
        let fuel = game.fuel;
        for _ in 0..10 {
            game.tick(Controls { up: true, ..Default::default() });
        }
        assert!(game.fuel < fuel, "fuel burns");
        assert!(game.scroll >= 10, "the world moves on");
        assert_eq!(game.screen().len(), (WIDTH * HEIGHT) as usize);
    }

    #[test]
    fn flying_into_the_ground_loses_a_pod_and_running_out_of_pods_ends_the_game() {
        let mut game = Game::new();
        game.tick(press(true));
        for _ in 0..3000 {
            if matches!(game.phase, Phase::Over(_)) {
                break;
            }
            game.tick(Controls { down: true, start: false, ..Default::default() });
        }
        assert!(matches!(game.phase, Phase::Over(_)), "{:?}", game.phase);
        assert_eq!(game.lives, 0);
    }

    #[test]
    fn hitting_a_canister_scores_and_refuels() {
        let mut game = Game::new();
        game.tick(press(true));
        game.fuel = 100;
        // A canister just in front of the pod's shots.
        let left = game.scroll * 16;
        game.course.things.push(Thing { kind: Kind::Canister, x: left + game.x + 30 * 16, y: game.y, vy: 0, age: 0, alive: true, hits: 0 });
        for _ in 0..10 {
            game.tick(Controls { fire: true, ..Default::default() });
        }
        assert!(game.score >= 150, "score {}", game.score);
        assert!(game.fuel > 300, "fuel {}", game.fuel);
    }

    #[test]
    fn every_stage_is_made_and_played_without_trouble() {
        for stage in 0..STAGES.len() {
            let made = course(stage);
            assert!(made.floor.iter().zip(&made.roof).all(|(f, r)| f - r >= 44), "stage {stage} leaves room to fly");
            assert!(made.things.iter().any(|t| t.kind == Kind::Canister), "stage {stage} has fuel");
        }
        // A wild pilot, pressing everything, through many frames: nothing goes wrong.
        let mut game = Game::new();
        let mut random = Random(7);
        for frame in 0..20_000u32 {
            let bits = random.next();
            let controls = Controls { up: bits & 1 != 0, down: bits & 2 != 0, left: bits & 4 != 0, right: bits & 8 != 0, fire: bits & 16 != 0, bomb: bits & 32 != 0, start: frame % 50 == 0 };
            game.tick(controls);
            if frame % 500 == 0 {
                let _ = game.screen();
            }
        }
    }

    #[test]
    fn bombing_the_core_wins() {
        let mut game = Game::new();
        game.tick(press(true));
        game.begin_stage(STAGES.len() - 1);
        let left = game.scroll * 16;
        // The core right under the pod's bombs.
        let core = game.course.things.iter_mut().find(|t| t.kind == Kind::Core).unwrap();
        core.x = left + game.x + 6 * 16;
        core.y = game.y + 20 * 16;
        let target = (core.x, core.y);
        for _ in 0..400 {
            if matches!(game.phase, Phase::Won(_)) {
                break;
            }
            // Keep the core where the bombs fall, and the pod clear of the ground.
            let left = game.scroll * 16;
            if let Some(core) = game.course.things.iter_mut().find(|t| t.kind == Kind::Core) {
                core.x = left + game.x + 6 * 16;
                core.y = target.1;
            }
            game.fuel = 1000;
            game.tick(Controls { bomb: true, ..Default::default() });
            if matches!(game.phase, Phase::Lost(_)) {
                game.phase = Phase::Playing;
                game.lives = 3;
            }
        }
        assert!(matches!(game.phase, Phase::Won(_)), "{:?}", game.phase);
    }

    /// Writes screens from each stage as pictures, to look at: CLAMBERS_SHOTS=<folder> cargo test -- --ignored shots
    #[test]
    #[ignore]
    fn shots() {
        let Ok(folder) = std::env::var("CLAMBERS_SHOTS") else { return };
        let save = |game: &Game, name: &str| {
            let mut ppm = format!("P6 {WIDTH} {HEIGHT} 255\n").into_bytes();
            for c in game.screen() {
                ppm.extend(PALETTE[c as usize]);
            }
            std::fs::write(format!("{folder}/{name}.ppm"), ppm).unwrap();
        };
        let mut game = Game::new();
        save(&game, "title");
        game.tick(press(true));
        for stage in 0..STAGES.len() {
            game.begin_stage(stage);
            game.phase = Phase::Playing;
            for frame in 0..400 {
                game.lives = 3;
                game.fuel = 800;
                game.phase = Phase::Playing;
                // Keep to the middle of the gap.
                let (roof, floor) = game.ground_at(game.x / 16 + 4);
                let middle = (roof + floor) / 2 * 16;
                game.tick(Controls { up: game.y > middle, down: game.y < middle, fire: frame % 3 == 0, bomb: frame % 20 == 0, ..Default::default() });
            }
            save(&game, &format!("stage{stage}"));
        }
    }
}
