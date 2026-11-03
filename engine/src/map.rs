//! A 32×32 tile grid. 1 KB fits a single storage bytestring and keeps
//! collision and line-of-sight to a handful of tile lookups.

pub const MAP_W: usize = 32;
pub const MAP_H: usize = 32;

pub const EMPTY: u8 = 0;
pub const BRICK: u8 = 1;
pub const STONE: u8 = 2;
pub const TECH: u8 = 3;

const ROWS: [&str; MAP_H] = [
    "################################",
    "#......#.........#.............#",
    "#......#.........#.............#",
    "#..P...%.........#....%%%%.....#",
    "#......%.......................#",
    "#......#.........#....%..%.....#",
    "###.####.........#....%..%.....#",
    "#.......#####.####.............#",
    "#..............................#",
    "#.......#####.####....@@@@@@...#",
    "###.#####.......#.....@....@...#",
    "#.....#.........#.....@....@...#",
    "#.....#...%%....#..........@...#",
    "#.....#...%%....#.....@@@@@@...#",
    "#...............#..............#",
    "#.....#.........######.#########",
    "#######.........#..............#",
    "#.....#.........#..............#",
    "#.....#####.#####...%......%...#",
    "#..............................#",
    "#.....#####.#####...%......%...#",
    "#.....#.........#..............#",
    "#.....#.........#..............#",
    "###.###.........######.#########",
    "#.....#.........#..............#",
    "#.....#.........#....@@..@@....#",
    "#...............#....@....@....#",
    "#.....#.........#....@....@....#",
    "#.....#........................#",
    "#.....#.........#..............#",
    "#.....#.........#..............#",
    "################################",
];

const fn parse(rows: &[&str; MAP_H]) -> ([u8; MAP_W * MAP_H], (i32, i32)) {
    let mut tiles = [EMPTY; MAP_W * MAP_H];
    let mut start = (-1, -1);
    let mut y = 0;
    while y < MAP_H {
        let row = rows[y].as_bytes();
        assert!(row.len() == MAP_W, "every map row must be exactly 32 tiles");
        let mut x = 0;
        while x < MAP_W {
            tiles[y * MAP_W + x] = match row[x] {
                b'#' => BRICK,
                b'%' => STONE,
                b'@' => TECH,
                b'P' => {
                    start = (x as i32, y as i32);
                    EMPTY
                }
                _ => EMPTY,
            };
            x += 1;
        }
        y += 1;
    }
    assert!(start.0 >= 0, "map needs a player start");
    (tiles, start)
}

const PARSED: ([u8; MAP_W * MAP_H], (i32, i32)) = parse(&ROWS);

pub static LEVEL: [u8; MAP_W * MAP_H] = PARSED.0;
pub const PLAYER_START: (i32, i32) = PARSED.1;

/// Out-of-bounds reads as wall, so rays and movement can never leave the map.
#[inline]
pub fn tile(x: i32, y: i32) -> u8 {
    if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 {
        BRICK
    } else {
        LEVEL[y as usize * MAP_W + x as usize]
    }
}
