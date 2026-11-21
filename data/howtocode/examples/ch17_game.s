; ch17_game.s - Gem Cave: a small but whole game. A cave of 48 by 40 tiles scrolls in eight directions behind the
; player, three bats fly about, and the gems are there to be collected. A title screen, the game, pause and game
; over, a score, lives, and a sound for each gem and each bite. A joystick in port 1 (the joystick's port) to play,
; fire to start and to pause. Left mouse button to leave.
;
; How To Code v8, chapter 17. CC0: use it for anything. The tiles, sprites, font and sounds are made by
; ch17_make_graphics.py, in ch17_graphics.raw.
;
; What's in it, and where the chapter explains it:
;   - a fixed-step game loop: the game moves in ticks of 1/50 second, on PAL and NTSC, however late a frame is
;   - the scrolling: a buffer only a little bigger than the screen, the copper setting the bitplane pointers at
;     the top and again where the buffer wraps round, and new tiles drawn in as they come into view
;   - a panel under the playfield, with its own bitplane and colours, by a copper split
;   - sprites for the player and the bats, clipped to the playfield, double-buffered with the copper list
;   - collisions: with the walls and gems from the tile map, with the bats from CLXDAT
;   - the joystick, with the fire button's press told from its being held
;   - a state machine: title, game, pause and game over
;   - the score in BCD, and sound effects started over two frames with no waiting
;
; $VER: ch17_game 8.0 (21.11.2026)

OS_FRIENDLY     equ     0
AUDIO_CHANNELS  equ     %1000           ; with the system running, channel 3 is allocated for the sound effects

        include "hardware.i"
        include "system.i"
        include "startup.i"

; ---- The map and the tiles
MAPW            equ     48              ; the cave, in tiles
MAPH            equ     40
TILE            equ     16              ; pixels
FLOOR           equ     0               ; the tile numbers, as ch17_graphics.raw has them
WALL            equ     1
GEM             equ     2
CRACKED         equ     3

; ---- The scroll buffer. Its lines are interleaved: each line holds a line of each of the four bitplanes in turn.
; A bitplane's line is 24 words: the 21 the display fetches, and room on each side for the column of tiles being
; drawn before it comes into view. 17 rows of tiles: the 14 or 15 on show, and one more above and below.
PLANES          equ     4
ROWBYTES        equ     48              ; one bitplane's line, in the buffer
LINEBYTES       equ     ROWBYTES*PLANES ; one line of the buffer: 192 bytes
SLOTS           equ     17              ; rows of tiles in the buffer
BUFLINES        equ     SLOTS*TILE      ; 272 lines
BUFBYTES        equ     BUFLINES*LINEBYTES+2*MAPW+4 ; and the drift: each 16 pixels right moves the screen 2 bytes on
FETCHBYTES      equ     42              ; 21 words a line: 20 on show, and one more for the fine scroll
PFMOD           equ     LINEBYTES-FETCHBYTES ; the playfield's modulo: from the end of a fetch to the next line
TILEBYTES       equ     TILE*PLANES*2   ; one tile in ch17_graphics.raw: 128 bytes
TILESIZE        equ     ((TILE*PLANES)<<6)!1 ; BLTSIZE: 64 rows of one word

; ---- The panel under the playfield: 32 lines of one bitplane, 21 words a line like the playfield's fetch.
PANELLINES      equ     32
PANELROW        equ     FETCHBYTES
PALPLAY         equ     224             ; the playfield's lines: PAL (256 in all)...
NTSCPLAY        equ     168             ; ...and NTSC (200)

; ---- Sprites: each is its control words, up to 16 lines of two words, and the two words that end it.
SPRBYTES        equ     4+16*4+4
ENEMIES         equ     3               ; on sprites 2, 4 and 6: even sprites, which CLXDAT always counts

; ---- The game
TICKRATE        equ     50              ; the game's steps a second, on PAL and NTSC alike
MAXTICKS        equ     4               ; the most steps one frame may catch up; after that the game slows down
SPEED           equ     2               ; the player's pixels a step
TITLE_STATE     equ     0
PLAY_STATE      equ     4               ; (times 4: offsets into the table of states)
PAUSE_STATE     equ     8
OVER_STATE      equ     12
SAFE_TICKS      equ     100             ; after a bite, two seconds before the next can hurt

; ---- ch17_graphics.raw, laid out as ch17_make_graphics.py says
GR_PALETTE      equ     0
GR_TILES        equ     32
GR_SPRITES      equ     544
GR_SPRCOLOURS   equ     736
GR_FONT         equ     1776
GR_PICKUP       equ     2288
PICKUPBYTES     equ     1600
GR_HURT         equ     3888
HURTBYTES       equ     3200
SOUNDPERIOD     equ     300

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        move.l  #GameExit,ExitHook

        ; PAL or NTSC: how tall the playfield is, where the display ends, and how often a frame comes.
        move.w  #PALPLAY,PlayLines
        move.w  #$2cc1,DiwStop
        move.w  #50,FrameRate
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #NTSCPLAY,PlayLines
        move.w  #$f4c1,DiwStop
        move.w  #60,FrameRate
.pal:
        move.l  Frames,LastFrames

        ; The panel starts empty (BSS memory isn't cleared on every Kickstart).
        lea     Panel,a0
        move.w  #PANELROW*PANELLINES/4-1,d0
.clear:
        clr.l   (a0)+
        dbf     d0,.clear

        move.l  #$2545f491,Seed         ; any number but 0; the press of fire mixes the time in
        bsr     NewLevel
        move.w  #TITLE_STATE,State
        bsr     ShowState
        move.w  #16,CamX
        move.w  #0,CamY
        move.w  #1,AttractX
        move.w  #1,AttractY

        ; The first frame's copper list and sprites, in the list the startup starts (Copper); the first Frame
        ; makes the other.
        bsr     Render
        bra     SwapLists

; ---------------------------------------------------------------------------------------------------------------
; As the game ends, while the sound effects' interrupt still runs: stop them, then the sound.

GameExit:
        st      SoundOff
        move.w  #DMAF_AUD3,DMACON(a5)
        move.w  #0,AUD3LC+8(a5)         ; AUD3VOL
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame. First the game moves on: one step for every 1/50 second since the last frame. On PAL that's one
; step a frame; on NTSC, five steps in every six frames; and after a frame that ran late, two or more. Then the
; screen is made to match: once, however many steps there were.

Frame:
        move.l  Frames,d0
        move.l  d0,d1
        sub.l   LastFrames,d0           ; frames since the last time
        move.l  d1,LastFrames
        cmp.l   #MAXTICKS,d0
        bls.s   .few
        moveq   #MAXTICKS,d0
.few:
        mulu    #TICKRATE,d0
        add.w   TickClock,d0            ; 50 for each frame...
        moveq   #0,d7
.count:
        cmp.w   FrameRate,d0            ; ...and a step for each 50 or 60 of them
        blo.s   .counted
        sub.w   FrameRate,d0
        addq.w  #1,d7
        bra.s   .count
.counted:
        move.w  d0,TickClock
        cmp.w   #MAXTICKS,d7
        bls.s   .steps
        moveq   #MAXTICKS,d7            ; too far behind: let the game slow down rather than never catch up
        clr.w   TickClock
.steps:
        bra.s   .next
.step:
        move.w  d7,-(sp)
        bsr     Tick
        move.w  (sp)+,d7
.next:
        dbf     d7,.step

        ; The bats touch the player: CLXDAT has noted it, as the sprites were shown since the last read.
        move.w  CLXDAT(a5),d0           ; reading it clears it
        cmp.w   #PLAY_STATE,State
        bne.s   .nobite
        and.w   #$0e00,d0               ; bits 9 to 11: sprite 0 met sprite 2, 4 or 6
        beq.s   .nobite
        tst.w   Safe                    ; just bitten: two seconds' grace
        bne.s   .nobite
        bsr     Bitten
.nobite:
        bsr     Render

        ; The copper list (and the sprites it points at) is shown from the next frame on. Not before: the copper
        ; reads COP1LC again at each vertical blank. The next frame's Frame makes the other list.
        move.l  BackCopper,COP1LC(a5)

; The list just made is the one on show; the other is the next to make.
SwapLists:
        move.l  BackCopper,d0
        move.l  ShownCopper,BackCopper
        move.l  d0,ShownCopper
        move.l  BackSprites,d0
        move.l  ShownSprites,BackSprites
        move.l  d0,ShownSprites
        rts

; ---------------------------------------------------------------------------------------------------------------
; One step of the game: the controls, then whatever the state does.

Tick:
        addq.w  #1,TickCount
        bsr     ReadJoystick
        move.w  State,d0                ; 0, 4, 8 or 12
        jmp     .states(pc,d0.w)
.states:
        bra.w   TitleTick
        bra.w   PlayTick
        bra.w   PauseTick
        bra.w   OverTick

TitleTick:
        bsr     MoveEnemies
        ; The camera wanders round the cave by itself.
        move.w  CamX,d0
        add.w   AttractX,d0
        cmp.w   #16,d0
        ble.s   .xturn
        cmp.w   #MAPW*TILE-320-16,d0
        blt.s   .xok
.xturn:
        neg.w   AttractX
.xok:
        move.w  d0,CamX
        move.w  CamY,d0
        add.w   AttractY,d0
        ble.s   .yturn
        move.w  #MAPH*TILE,d1
        sub.w   PlayLines,d1
        cmp.w   d1,d0
        blt.s   .yok
.yturn:
        neg.w   AttractY
.yok:
        move.w  d0,CamY
        tst.b   FirePressed
        beq.s   .wait
        bsr     StartGame
.wait:
        rts

PlayTick:
        tst.b   FirePressed
        beq.s   .play
        move.w  #PAUSE_STATE,State
        bra     ShowState
.play:
        tst.w   Safe
        beq.s   .unsafe
        subq.w  #1,Safe
.unsafe:
        bsr     MovePlayer
        bsr     TakeGem
        bsr     MoveEnemies
        bra     FollowPlayer

PauseTick:
        tst.b   FirePressed
        beq.s   .paused
        move.w  #PLAY_STATE,State
        bra     ShowState
.paused:
        rts

OverTick:
        bsr     MoveEnemies
        subq.w  #1,OverTime
        beq.s   .title
        cmp.w   #100,OverTime           ; a second to see it before fire skips it
        bhs.s   .stay
        tst.b   FirePressed
        beq.s   .stay
.title:
        move.w  #TITLE_STATE,State
        bsr     NewLevel                ; the gems back for the title screen
        bra     ShowState
.stay:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The joystick in port 1 (JOY1DAT; see chapter 11). JoyX and JoyY are -1, 0 or 1. Fire is held, FirePressed is
; only this step's new press: a game wants to know that fire went down, not that it's down, or one press starts
; the game and then pauses it at once.

ReadJoystick:
        move.w  JOY1DAT(a5),d0
        move.w  d0,d1
        lsr.w   #1,d1
        eor.w   d0,d1                   ; bit 8 up, bit 0 down
        moveq   #0,d2
        btst    #1,d0
        beq.s   .notright
        addq.w  #1,d2
.notright:
        btst    #9,d0
        beq.s   .notleft
        subq.w  #1,d2
.notleft:
        moveq   #0,d3
        btst    #0,d1
        beq.s   .notdown
        addq.w  #1,d3
.notdown:
        btst    #8,d1
        beq.s   .notup
        subq.w  #1,d3
.notup:
        move.w  d2,JoyX
        move.w  d3,JoyY
        move.b  Fire,d1                 ; held last step
        btst    #CIAB_GAMEPORT1,CIAA+CIAPRA
        seq     d0                      ; held now (the bit is 0 while it's pressed)
        move.b  d0,Fire
        not.b   d1
        and.b   d1,d0                   ; held now and not before
        move.b  d0,FirePressed
        rts

; ---------------------------------------------------------------------------------------------------------------
; The game starts: the press of fire mixes the time into the random numbers (people never press it at the same
; moment twice), a new cave, three lives.

StartGame:
        move.w  VHPOSR(a5),d0
        swap    d0
        move.w  TickCount,d0
        eor.l   d0,Seed
        bne.s   .seeded
        move.l  #$2545f491,Seed         ; xorshift must never be 0, or it stays 0
.seeded:
        bsr     NewLevel
        move.w  #3,Lives
        clr.l   Score
        clr.w   Safe
        move.w  #PLAY_STATE,State
        bsr     ShowState
        bra     FollowPlayer

; A new cave: the map made into tile numbers from the text below, the gems counted, the player and the bats put
; where the map says.
NewLevel:
        lea     MapText,a0
        lea     MapTiles,a1
        lea     Enemies,a2
        moveq   #0,d5                   ; gems
        moveq   #0,d1                   ; row
.row:
        moveq   #0,d0                   ; column
.column:
        move.b  (a0)+,d2
        moveq   #WALL,d3
        cmp.b   #'#',d2
        beq.s   .put
        moveq   #GEM,d3
        cmp.b   #'*',d2
        bne.s   .notgem
        addq.w  #1,d5
        bra.s   .put
.notgem:
        moveq   #CRACKED,d3
        cmp.b   #',',d2
        beq.s   .put
        moveq   #FLOOR,d3
        cmp.b   #'P',d2
        bne.s   .notplayer
        move.w  d0,d4
        lsl.w   #4,d4
        move.w  d4,PlayerX
        move.w  d1,d4
        lsl.w   #4,d4
        move.w  d4,PlayerY
        bra.s   .put
.notplayer:
        cmp.b   #'E',d2
        bne.s   .put
        move.w  d0,d4
        lsl.w   #4,d4
        move.w  d4,(a2)+                ; x
        move.w  d1,d4
        lsl.w   #4,d4
        move.w  d4,(a2)+                ; y
        move.w  #1,(a2)+                ; moving right...
        move.w  #-1,(a2)+               ; ...and up
.put:
        move.b  d3,(a1)+
        addq.w  #1,d0
        cmp.w   #MAPW,d0
        blo.s   .column
        addq.w  #1,d1
        cmp.w   #MAPH,d1
        blo.s   .row
        move.w  d5,GemsLeft
        move.w  #-1000,DrawnRow         ; the whole buffer is drawn again, at the next Render
        rts

; ---------------------------------------------------------------------------------------------------------------
; The player moves, across and down separately: so a wall stops one direction, and the player slides along it
; in the other, instead of sticking to it.

MovePlayer:
        move.w  JoyX,d0
        muls    #SPEED,d0
        add.w   PlayerX,d0              ; across...
        move.w  PlayerY,d1
        lea     PlayerBox(pc),a1
        bsr     HitsWall
        bne.s   .noacross
        move.w  d0,PlayerX
.noacross:
        move.w  JoyY,d1
        muls    #SPEED,d1
        add.w   PlayerY,d1              ; ...then down, from where across left it
        move.w  PlayerX,d0
        lea     PlayerBox(pc),a1
        bsr     HitsWall
        bne.s   .nodown
        move.w  d1,PlayerY
.nodown:
        rts

; The part of each sprite that counts for walls: left, top, right and bottom, from its top left corner.
PlayerBox:      dc.w    4,2,11,15
EnemyBox:       dc.w    1,4,14,13

; Whether a box at (d0, d1), in the cave's pixels, is on a wall: a box no bigger than a tile can only touch the
; tiles under its four corners. Returns Z clear (bne) if it is. Keeps d0 and d1.
HitsWall:
        movem.l d0-d1,-(sp)
        move.w  d0,d2
        add.w   (a1),d2                 ; left
        move.w  d1,d3
        add.w   2(a1),d3                ; top
        bsr     IsWall
        bne.s   .done
        move.w  d0,d2
        add.w   4(a1),d2                ; right
        bsr     IsWall
        bne.s   .done
        move.w  d1,d3
        add.w   6(a1),d3                ; bottom
        bsr     IsWall
        bne.s   .done
        move.w  d0,d2
        add.w   (a1),d2                 ; left
        bsr     IsWall
.done:
        movem.l (sp)+,d0-d1             ; (movem to registers leaves the flags alone)
        rts

; Whether the cave's pixel (d2, d3) is in a wall: Z clear if it is.
IsWall:
        bsr     TileAt
        cmp.b   #WALL,(a0)
        bne.s   .floor
        moveq   #1,d4                   ; Z clear
        rts
.floor:
        moveq   #0,d4                   ; Z set
        rts

; The tile at the cave's pixel (d2, d3): a0 points at it in MapTiles. Outside the map counts as its corner.
TileAt:
        move.w  d3,d4
        asr.w   #4,d4
        mulu    #MAPW,d4
        move.w  d2,d5
        asr.w   #4,d5
        add.w   d5,d4
        lea     MapTiles,a0
        add.w   d4,a0
        rts

; ---------------------------------------------------------------------------------------------------------------
; A gem under the middle of the player is taken: the map and the screen show floor there from now on.

TakeGem:
        move.w  PlayerX,d2
        addq.w  #8,d2
        move.w  PlayerY,d3
        addq.w  #8,d3
        bsr     TileAt
        cmp.b   #GEM,(a0)
        bne.s   .none
        move.b  #FLOOR,(a0)
        lsr.w   #4,d2
        lsr.w   #4,d3
        bsr     RedrawTile
        lea     Ten(pc),a0
        bsr     AddScore
        lea     Graphics+GR_PICKUP,a0
        move.w  #PICKUPBYTES/2,d0
        bsr     PlaySound
        subq.w  #1,GemsLeft
        bne.s   .more
        lea     ClearedText(pc),a0
        bra     GameOver
.more:
        st      PanelChanged
.none:
        rts

Ten:    dc.b    $00,$00,$10             ; 10 points, in BCD
        even

; The score is kept in BCD: six decimal digits in three bytes, two to a byte. ABCD adds them a byte at a time,
; carrying in X; and printing them needs no division. a0: the points, in the same form.
AddScore:
        lea     Score+3,a1              ; from the end: the lowest digits first
        addq.l  #3,a0                   ; a0: the points, in BCD too
        sub.b   d0,d0                   ; X clear (SUB sets X as it sets C: no borrow)
        abcd    -(a0),-(a1)
        abcd    -(a0),-(a1)
        abcd    -(a0),-(a1)
        st      PanelChanged
        rts

Bitten:
        move.w  #SAFE_TICKS,Safe
        lea     Graphics+GR_HURT,a0
        move.w  #HURTBYTES/2,d0
        bsr     PlaySound
        st      PanelChanged
        subq.w  #1,Lives
        bne.s   .alive
        lea     GameOverText(pc),a0
        bra     GameOver
.alive:
        rts

GameOver:
        move.l  a0,OverText
        move.w  #OVER_STATE,State
        move.w  #150,OverTime           ; three seconds
        bra     ShowState

; ---------------------------------------------------------------------------------------------------------------
; The bats fly diagonally, a pixel a step. At a wall a bat turns: back the way it came on that axis, or, one time
; in four, a new direction picked at random on the other too, so they don't fly the same paths for ever.

MoveEnemies:
        lea     Enemies,a2
        moveq   #ENEMIES-1,d7
.enemy:
        movem.w (a2),d0-d1
        add.w   4(a2),d0
        lea     EnemyBox(pc),a1
        bsr     HitsWall
        beq.s   .acrossok
        neg.w   4(a2)
        bsr     MaybeTurn
        move.w  (a2),d0                 ; it stays where it was, across
.acrossok:
        add.w   6(a2),d1
        bsr     HitsWall
        beq.s   .downok
        neg.w   6(a2)
        bsr     MaybeTurn
        move.w  2(a2),d1                ; and down
.downok:
        movem.w d0-d1,(a2)
        addq.l  #8,a2
        dbf     d7,.enemy
        rts

MaybeTurn:
        movem.l d0-d1,-(sp)
        bsr     Random
        and.w   #3,d0                   ; the low bits of a xorshift are as good as the high ones
        bne.s   .keep
        bsr     Random
        moveq   #1,d2
        btst    #0,d0
        beq.s   .x
        neg.w   d2
.x:
        move.w  d2,4(a2)
        moveq   #1,d2
        btst    #1,d0
        beq.s   .y
        neg.w   d2
.y:
        move.w  d2,6(a2)
.keep:
        movem.l (sp)+,d0-d1
        rts

; A random number: xorshift32, George Marsaglia's (2003). Three shifts and three EORs, a new number in d0 each
; time, all of them before it repeats. Changes d0-d2.
Random:
        move.l  Seed,d0
        move.l  d0,d1
        moveq   #13,d2
        lsl.l   d2,d1
        eor.l   d1,d0                   ; x ^= x << 13
        move.l  d0,d1
        moveq   #17,d2
        lsr.l   d2,d1
        eor.l   d1,d0                   ; x ^= x >> 17
        move.l  d0,d1
        lsl.l   #5,d1
        eor.l   d1,d0                   ; x ^= x << 5
        move.l  d0,Seed
        rts

; ---------------------------------------------------------------------------------------------------------------
; The camera keeps the player in the middle, except where the cave ends. A column of tiles is kept out of view
; on the left (CamX at least 16), so the column being drawn there is never seen.

FollowPlayer:
        move.w  PlayerX,d0
        sub.w   #160-8,d0
        cmp.w   #16,d0
        bge.s   .notleft
        moveq   #16,d0
.notleft:
        cmp.w   #MAPW*TILE-320-16,d0
        ble.s   .notright
        move.w  #MAPW*TILE-320-16,d0
.notright:
        move.w  d0,CamX
        move.w  PlayLines,d2
        lsr.w   #1,d2
        move.w  PlayerY,d0
        addq.w  #8,d0
        sub.w   d2,d0
        bge.s   .nottop
        moveq   #0,d0
.nottop:
        move.w  #MAPH*TILE,d1
        sub.w   PlayLines,d1
        cmp.w   d1,d0
        ble.s   .notbottom
        move.w  d1,d0
.notbottom:
        move.w  d0,CamY
        rts

; ---------------------------------------------------------------------------------------------------------------
; Sound effects, on channel 3, started over two frames from the vertical blank, so nothing waits for Paula:
; one frame to stop the channel (it may still be playing the last effect, and must see its DMA off for at least
; two samples, chapter 12), the next to start the new one, and the next to point it at silence, to play once.
; a0: the sound, d0: its length in words.

PlaySound:
        move.w  d0,SoundLength
        move.l  a0,SoundNext            ; last: a long is written in one go, so the interrupt sees both or neither
        rts

VBlank:
        tst.b   SoundOff
        bne.s   .done
        move.w  SoundStep,d0
        beq.s   .idle
        subq.w  #1,d0
        beq.s   .start
        ; Step 2: it started a frame ago, so the channel has its own copy of the address and length by now.
        move.l  #Silence,AUD3LC(a5)
        move.w  #1,AUD3LC+4(a5)         ; AUD3LEN
        clr.w   SoundStep
        rts
.start:
        ; Step 1: the channel has been off for a frame. Set it up and start it.
        move.l  SoundPlaying,AUD3LC(a5)
        move.w  SoundLength,AUD3LC+4(a5)
        move.w  #SOUNDPERIOD,AUD3LC+6(a5) ; AUD3PER
        move.w  #64,AUD3LC+8(a5)        ; AUD3VOL
        move.w  #DMAF_SETCLR!DMAF_MASTER!DMAF_AUD3,DMACON(a5)
        move.w  #2,SoundStep
        rts
.idle:
        move.l  SoundNext,d0
        beq.s   .done
        clr.l   SoundNext
        move.l  d0,SoundPlaying
        move.w  #DMAF_AUD3,DMACON(a5)   ; step 0: off
        move.w  #1,SoundStep
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The screen, made to match the game: the tiles that have come into view, the copper list, the sprites, the panel.

Render:
        bsr     Scroll
        bsr     MakeCopper
        bsr     MakeSprites
        tst.b   PanelChanged
        beq.s   .panel
        bsr     DrawPanel
.panel:
        rts

; The scroll: where the screen is in the buffer, and the tiles that are new to it. Each axis moves at most a tile
; a frame, so at most one new column and one new row are drawn; anything more (a new game, a new cave) draws
; all 17 rows.
Scroll:
        move.w  CamX,d0
        subq.w  #1,d0                   ; the screen's first pixel is the fetch's 17th, at a delay of 15
        move.w  d0,d1
        asr.w   #4,d0
        move.w  d0,ScrollWord           ; the first word fetched: the tile column on the left
        and.w   #15,d1
        moveq   #15,d2
        sub.w   d1,d2
        move.w  d2,d1
        lsl.w   #4,d1
        or.w    d2,d1
        move.w  d1,ScrollDelay          ; BPLCON1: both playfields' delay
        move.w  CamY,d1
        move.w  d1,d2
        asr.w   #4,d1                   ; the tile row at the top
        ext.l   d2
        divu    #BUFLINES,d2
        swap    d2
        move.w  d2,BufferTop            ; the buffer's line at the top of the screen

        BLIT_BEGIN
        bsr     TileBlits
        move.w  ScrollWord,d0
        move.w  CamY,d1
        asr.w   #4,d1
        move.w  DrawnRow,d2
        sub.w   d1,d2
        addq.w  #1,d2
        cmp.w   #2,d2
        bhi.s   .all                    ; more than a row away (or never drawn)
        move.w  DrawnColumn,d2
        sub.w   d0,d2
        addq.w  #1,d2
        cmp.w   #2,d2
        bhi.s   .all
        ; A new column first, for the rows drawn till now...
        cmp.w   DrawnColumn,d0
        beq.s   .rows
        blt.s   .left
        move.w  d0,d2
        add.w   #21,d2                  ; moving right: the column past the right edge
        bra.s   .column
.left:
        move.w  d0,d2
        subq.w  #1,d2                   ; moving left: the one past the left edge
.column:
        move.w  d0,DrawnColumn
        bsr     DrawColumn
.rows:
        ; ...then a new row, as wide as the new columns.
        move.w  CamY,d1
        asr.w   #4,d1
        cmp.w   DrawnRow,d1
        beq.s   .drawn
        blt.s   .up
        move.w  d1,d2
        add.w   #15,d2                  ; moving down: the row below the bottom one
        bra.s   .row
.up:
        move.w  d1,d2
        subq.w  #1,d2                   ; moving up: the row above the top
.row:
        move.w  d1,DrawnRow
        bsr     DrawRow
        bra.s   .drawn
.all:
        move.w  d0,DrawnColumn
        move.w  d1,DrawnRow
        move.w  d1,d2
        subq.w  #1,d2
        moveq   #SLOTS-1,d7
.every:
        bsr     DrawRow
        addq.w  #1,d2
        dbf     d7,.every
.drawn:
        BLIT_END
        rts

; Row d2 of the cave, from the column left of the screen to the one right of it (23 tiles). Keeps d2 and d7.
DrawRow:
        movem.l d2/d7,-(sp)
        move.w  DrawnColumn,d3
        subq.w  #1,d3
        moveq   #23-1,d7
.tile:
        move.w  d2,d0
        move.w  d3,d1
        bsr     DrawTile
        addq.w  #1,d3
        dbf     d7,.tile
        movem.l (sp)+,d2/d7
        rts

; Column d2 of the cave, from the row above the screen to the one below it (17 tiles).
DrawColumn:
        move.w  DrawnRow,d3
        subq.w  #1,d3
        moveq   #SLOTS-1,d7
.tile:
        move.w  d3,d0
        move.w  d2,d1
        bsr     DrawTile
        addq.w  #1,d3
        dbf     d7,.tile
        rts

; The blitter's settings for tiles: A to D, a word wide, D's modulo the rest of the buffer's line.
TileBlits:
        jsr     WaitBlitter
        move.l  #$09f00000,BLTCON0(a5)  ; A and D, D = A; BLTCON1 0
        move.l  #$ffffffff,BLTAFWM(a5)
        move.w  #0,BLTAMOD(a5)
        move.w  #ROWBYTES-2,BLTDMOD(a5)
        rts

; The tile at row d0, column d1 of the cave, into the buffer, where the screen will find it. A row of tiles lives
; in slot (row mod 17); a column at 2*(column+1) bytes into each line. The buffer's lines follow each other in
; memory, so as the screen moves right through them, what's right of one line is the start of the next: the
; display, the tiles and the blitter all see the same long ribbon, and need no wrapping across.
DrawTile:
        cmp.w   #MAPH,d0
        bhs.s   .outside                ; (unsigned: a row of -1 is outside too)
        cmp.w   #MAPW,d1
        bhs.s   .outside
        move.w  d0,d4
        mulu    #MAPW,d4
        add.w   d1,d4
        lea     MapTiles,a0
        moveq   #0,d5
        move.b  (a0,d4.w),d5
        lsl.w   #7,d5                   ; 128 bytes a tile
        lea     Graphics+GR_TILES,a0
        add.w   d5,a0
        moveq   #0,d4
        move.w  d0,d4
        divu    #SLOTS,d4
        swap    d4                      ; the slot
        mulu    #TILE*LINEBYTES,d4
        lea     Buffer,a1
        add.l   d4,a1
        move.w  d1,d4
        addq.w  #1,d4
        add.w   d4,d4
        add.w   d4,a1
        jsr     WaitBlitter
        move.l  a0,BLTAPTH(a5)
        move.l  a1,BLTDPTH(a5)
        move.w  #TILESIZE,BLTSIZE(a5)
.outside:
        rts

; A tile that's changed in the map (a gem taken), at row d3, column d2, drawn again if the buffer has it now.
RedrawTile:
        move.w  d3,d0
        sub.w   DrawnRow,d0
        addq.w  #1,d0
        cmp.w   #SLOTS,d0
        bhs.s   .notdrawn
        move.w  d2,d0
        sub.w   DrawnColumn,d0
        addq.w  #1,d0
        cmp.w   #23,d0
        bhs.s   .notdrawn
        BLIT_BEGIN
        bsr     TileBlits
        move.w  d3,d0
        move.w  d2,d1
        bsr     DrawTile
        BLIT_END
.notdrawn:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The copper list for the next frame, made afresh each time: its waits move with the scroll. Each copper WAIT
; compares only the low 8 bits of the line, so a wait for line 256 or more needs a wait for the end of line 255
; first ($FFDF), once (a second one, past line 255, would wait until the next frame).

MakeCopper:
        move.l  BackCopper,a0
        moveq   #0,d6                   ; not past line 255 yet
        move.l  #(FMODE<<16)!$0000,(a0)+ ; AGA: plain fetches, as OCS and ECS have
        move.l  #(BPLCON3<<16)!$0c00,(a0)+
        move.l  #(DIWSTRT<<16)!$2c81,(a0)+
        move.w  #DIWSTOP,(a0)+
        move.w  DiwStop,(a0)+
        move.l  #(DDFSTRT<<16)!$0030,(a0)+ ; a word early, for the fine scroll (it costs sprite 7)
        move.l  #(DDFSTOP<<16)!$00d0,(a0)+
        move.l  #(BPLCON0<<16)!$4200,(a0)+ ; four bitplanes
        move.w  #BPLCON1,(a0)+
        move.w  ScrollDelay,(a0)+
        move.l  #(BPLCON2<<16)!$0024,(a0)+ ; the sprites in front of the playfield
        move.l  #(BPL1MOD<<16)!PFMOD,(a0)+
        move.l  #(BPL2MOD<<16)!PFMOD,(a0)+
        lea     Graphics+GR_PALETTE,a1
        move.w  #COLOR00,d0
        moveq   #16-1,d1
.colour:
        move.w  d0,(a0)+
        move.w  (a1)+,(a0)+
        addq.w  #2,d0
        dbf     d1,.colour
        ; The sprites' colours: the player's (17-19, for sprites 0 and 1), the bats' (21-23, 25-27, 29-31).
        lea     Graphics+GR_SPRCOLOURS,a1
        move.w  #COLOR17,d0
        bsr     CopperColours
        move.w  #COLOR21,d0
        bsr     CopperBatColours
        move.w  #COLOR25,d0
        bsr     CopperBatColours
        move.w  #COLOR29,d0
        bsr     CopperBatColours

        ; The sprite pointers: 0 the player, 2, 4 and 6 the bats, the rest empty.
        move.l  BackSprites,d0
        lea     NoSprite,a1
        move.l  a1,d1
        move.w  #SPR0PTH,d2
        moveq   #4-1,d3
.sprites:
        bsr     CopperLong              ; the even sprite: one of ours
        add.l   #SPRBYTES,d0
        exg     d0,d1
        bsr     CopperLong              ; the odd one: empty
        exg     d0,d1
        dbf     d3,.sprites
        move.l  #(DMACON<<16)!DMAF_SETCLR!DMAF_SPRITE,(a0)+ ; sprite DMA on, now its pointers are right

        ; The bitplanes at the top of the screen.
        move.w  BufferTop,d0
        mulu    #LINEBYTES,d0
        add.l   #Buffer,d0
        move.w  ScrollWord,d1
        addq.w  #1,d1
        add.w   d1,d1
        ext.l   d1
        add.l   d1,d0
        bsr     CopperPlanes

        ; Where the buffer runs out, back to its first line, if that's on the screen.
        move.w  #BUFLINES,d0
        sub.w   BufferTop,d0            ; the screen's line that shows the buffer's line 0
        cmp.w   PlayLines,d0
        bhs.s   .nowrap
        bsr     CopperWait
        move.l  #Buffer,d0
        move.w  ScrollWord,d1
        addq.w  #1,d1
        add.w   d1,d1
        ext.l   d1
        add.l   d1,d0
        bsr     CopperPlanes
.nowrap:
        ; The panel: one bitplane, not scrolled, colours of its own.
        move.w  PlayLines,d0
        bsr     CopperWait
        move.l  #Panel,d0
        move.w  #BPL1PTH,d2
        bsr     CopperLong
        move.l  #(BPLCON0<<16)!$1200,(a0)+
        move.l  #(BPLCON1<<16)!$0000,(a0)+
        move.l  #(BPL1MOD<<16)!$0000,(a0)+
        move.l  #(COLOR00<<16)!$0124,(a0)+
        move.l  #(COLOR01<<16)!$0ffc,(a0)+
        move.l  #$fffffffe,(a0)+        ; the end: a wait for a line that never comes
        rts

; Three colours into the copper list, from a1, to the registers from d0 on.
CopperBatColours:
        lea     Graphics+GR_SPRCOLOURS+6,a1
CopperColours:
        moveq   #3-1,d1
.colour:
        move.w  d0,(a0)+
        move.w  (a1)+,(a0)+
        addq.w  #2,d0
        dbf     d1,.colour
        rts

; A pointer pair into the copper list: d2 the high word's register (then d2 is the next pair's), d0 the address.
CopperLong:
        swap    d0
        move.w  d2,(a0)+
        move.w  d0,(a0)+
        addq.w  #2,d2
        swap    d0
        move.w  d2,(a0)+
        move.w  d0,(a0)+
        addq.w  #2,d2
        rts

; The four bitplane pointers, from d0 (plane 0; each next plane's line follows in the interleaved buffer).
CopperPlanes:
        move.w  #BPL1PTH,d2
        moveq   #PLANES-1,d3
.plane:
        bsr     CopperLong
        add.l   #ROWBYTES,d0
        dbf     d3,.plane
        rts

; A wait for the start of the screen's line d0 (0 is the playfield's first), with $FFDF first when it's needed.
CopperWait:
        add.w   #$2c,d0                 ; the screen's line, as the beam counts
        cmp.w   #$100,d0
        blo.s   .low
        tst.w   d6                      ; past 255 already?
        bne.s   .low
        move.l  #$ffdffffe,(a0)+
        moveq   #1,d6
.low:
        lsl.w   #8,d0
        or.w    #$07,d0                 ; at the left edge, before the line's fetch starts
        move.w  d0,(a0)+
        move.w  #$fffe,(a0)+
        rts

; ---------------------------------------------------------------------------------------------------------------
; The sprites for the next frame: their control words, and their lines copied in, cut to the playfield. Sprites
; are cut by the display window (the HRM: "sprites will be partially or totally clipped if they pass across or
; beyond the boundaries of the display window"), but the panel is inside it, so the bottom is cut here; and the
; top too, so no sprite starts before the display does.

MakeSprites:
        move.l  BackSprites,a3
        ; The player: hidden on the title screen, and flashing while it's safe after a bite.
        move.w  PlayerX,d0
        sub.w   CamX,d0
        move.w  PlayerY,d1
        sub.w   CamY,d1
        lea     Graphics+GR_SPRITES,a1
        cmp.w   #TITLE_STATE,State
        beq.s   .hideplayer
        btst    #2,Safe+1
        beq.s   .player
.hideplayer:
        move.w  #-100,d1                ; off the top: not shown
.player:
        bsr     MakeSprite
        ; The bats, flapping every 8 steps.
        lea     Enemies,a2
        moveq   #ENEMIES-1,d7
.bat:
        add.l   #SPRBYTES,a3
        move.w  (a2),d0
        sub.w   CamX,d0
        move.w  2(a2),d1
        sub.w   CamY,d1
        lea     Graphics+GR_SPRITES+64,a1
        btst    #3,TickCount+1
        beq.s   .frame
        add.w   #64,a1
.frame:
        bsr     MakeSprite
        addq.l  #8,a2
        dbf     d7,.bat
        rts

; A sprite at a3, showing the image at a1 (16 lines) at (d0, d1) on the screen.
MakeSprite:
        moveq   #0,d2                   ; the first line shown...
        moveq   #16,d3                  ; ...and the one after the last
        tst.w   d1
        bge.s   .top
        move.w  d1,d2
        neg.w   d2                      ; above the top: start lower in the image
.top:
        move.w  PlayLines,d4
        sub.w   d1,d4
        cmp.w   d4,d3
        ble.s   .bottom
        move.w  d4,d3                   ; past the bottom: end sooner
.bottom:
        cmp.w   d3,d2
        bge.s   .hidden
        cmp.w   #-16,d0
        ble.s   .hidden
        cmp.w   #320,d0
        bge.s   .hidden
        ; The control words. VSTART and VSTOP are 9 bits, HSTART 9; their top bits are in SPRxCTL.
        move.w  d1,d4
        add.w   d2,d4
        add.w   #$2c,d4                 ; VSTART
        move.w  d1,d5
        add.w   d3,d5
        add.w   #$2c,d5                 ; VSTOP: the line after the last
        add.w   #$80,d0                 ; HSTART
        move.w  d4,d6
        lsl.w   #8,d6
        move.w  d0,d1
        lsr.w   #1,d1
        move.b  d1,d6
        move.w  d6,(a3)                 ; SPRxPOS: VSTART's low 8 bits, HSTART's high 8
        move.w  d5,d6
        lsl.w   #8,d6
        btst    #8,d4
        beq.s   .v8
        bset    #2,d6
.v8:
        btst    #8,d5
        beq.s   .e8
        bset    #1,d6
.e8:
        btst    #0,d0
        beq.s   .h0
        bset    #0,d6
.h0:
        move.w  d6,2(a3)                ; SPRxCTL
        lea     4(a3),a0
        move.w  d2,d4
        lsl.w   #2,d4
        add.w   d4,a1                   ; the first line shown
        sub.w   d2,d3
        subq.w  #1,d3
.line:
        move.l  (a1)+,(a0)+
        dbf     d3,.line
        clr.l   (a0)                    ; the end
        rts
.hidden:
        clr.l   (a3)                    ; control words of 0: the sprite ends before it starts
        rts

; ---------------------------------------------------------------------------------------------------------------
; The panel's words: the score, lives and gems on the top line, the state's message under them.

ShowState:
        move.w  State,d0
        lea     Messages(pc),a0
        move.l  (a0,d0.w),a0
        cmp.w   #OVER_STATE,State
        bne.s   .message
        move.l  OverText,a0
.message:
        move.l  a0,Message
        st      PanelChanged
        rts

DrawPanel:
        sf      PanelChanged
        lea     ScoreText(pc),a0
        moveq   #1,d0
        moveq   #4,d1
        bsr     Print
        ; The score's six BCD digits, a nibble each.
        lea     Score,a2
        moveq   #7,d2                   ; the column
        moveq   #3-1,d3
.byte:
        move.b  (a2),d4
        lsr.b   #4,d4
        bsr     .digit
        move.b  (a2)+,d4
        and.b   #15,d4
        bsr     .digit
        dbf     d3,.byte
        move.w  Lives,d4
        moveq   #29,d2
        bsr     .digit
        move.w  GemsLeft,d4
        ext.l   d4
        divu    #10,d4
        moveq   #37,d2
        bsr     .digit
        swap    d4
        bsr     .digit
        lea     Blank(pc),a0
        moveq   #0,d0
        moveq   #18,d1
        bsr     Print
        move.l  Message,a0
        moveq   #0,d0
.length:
        tst.b   (a0,d0.w)
        beq.s   .centre
        addq.w  #1,d0
        bra.s   .length
.centre:
        neg.w   d0
        add.w   #40,d0
        lsr.w   #1,d0
        moveq   #18,d1
        bra     Print
.digit:
        lea     DigitText,a0
        add.b   #'0',d4
        move.b  d4,(a0)
        move.w  d2,d0
        moveq   #4,d1
        bsr     Print
        addq.w  #1,d2
        rts

; Prints the text at a0 (ending with 0) at column d0 (of 40) on the panel's line d1, by the processor: each
; character is 8 bytes of the font, one to a line.
Print:
        lea     Panel+2,a1              ; past the word the fine scroll would show (the panel's delay is 0)
        mulu    #PANELROW,d1
        add.l   d1,a1
        add.w   d0,a1
.char:
        moveq   #0,d0
        move.b  (a0)+,d0
        beq.s   .done
        sub.b   #32,d0
        lsl.w   #3,d0
        lea     Graphics+GR_FONT,a4
        add.w   d0,a4
        move.l  a1,a6
        moveq   #8-1,d1
.line:
        move.b  (a4)+,(a6)
        add.w   #PANELROW,a6
        dbf     d1,.line
        addq.l  #1,a1
        bra.s   .char
.done:
        rts

Messages:
        dc.l    TitleText,PlayText,PauseText,0
ScoreText:      dc.b    "SCORE 000000          LIVES 0  GEMS 00",0
TitleText:      dc.b    "GEM CAVE  -  PRESS FIRE TO PLAY",0
PlayText:       dc.b    "FIRE TO PAUSE",0
PauseText:      dc.b    "PAUSED  -  FIRE TO PLAY ON",0
GameOverText:   dc.b    "GAME OVER",0
ClearedText:    dc.b    "CAVE CLEARED!",0
Blank:          dc.b    "                                        ",0
        even

; ---------------------------------------------------------------------------------------------------------------

        section data,data

; The cave: # wall, * gem, , cracked floor, P the player, E a bat. 48 by 40, one line of text for each row.
MapText:
        dc.b    "################################################"
        dc.b    "#P.....,.......#...........*........#.........*#"
        dc.b    "#..............#....................#..........#"
        dc.b    "#...*....####..#....#######.........#...####...#"
        dc.b    "#........#.....#....#.....#....*....#......#...#"
        dc.b    "#...,....#..*..#....#..*..#.........#..*...#...#"
        dc.b    "#........#.....,....#.....#.........,......#...#"
        dc.b    "#####..###..........##..###....######......#...#"
        dc.b    "#.........................................##...#"
        dc.b    "#..*.......E.............*....................*#"
        dc.b    "#..............######.........,.........#......#"
        dc.b    "#....######....#....#...................#......#"
        dc.b    "#....#....#....#.*..#.....#####.....*...#####..#"
        dc.b    "#....#.*..#....#....#.....#...#................#"
        dc.b    "#....#....#.........#.....#.*.#................#"
        dc.b    "#....##.###....######.....#...#.....######.....#"
        dc.b    "#..........................#.##.....#....#.....#"
        dc.b    "#..*.......,...................................#"
        dc.b    "#.................E................#.*..#..*...#"
        dc.b    "######.....####.......#######......#....#......#"
        dc.b    "#....#.....#..#.......#.....#......######......#"
        dc.b    "#.*..#.....#*.#..,....#..*..#..................#"
        dc.b    "#....#.....#..#.......#.....#.......,..........#"
        dc.b    "#....###.###..##.######.....#####.....####.....#"
        dc.b    "#..............................*......#..#.....#"
        dc.b    "#.....*...............................#*.#..*..#"
        dc.b    "#...........#######...................#..#.....#"
        dc.b    "#####.......#.....#.....######.........#.#.....#"
        dc.b    "#...#.......#..*..#.....#....#.................#"
        dc.b    "#.*.#.......#.....#.....#.*..#.....E...........#"
        dc.b    "#...#...,...###.###.....#....#.......######....#"
        dc.b    "#...#...................##.###.......#....#....#"
        dc.b    "#..........*.........................#.*..#....#"
        dc.b    "#..................######............#....#....#"
        dc.b    "#....####..........#....#....*.......###.##....#"
        dc.b    "#....#..#..........#..*.#......................#"
        dc.b    "#....#*.#....,.....#....#.......,..........*...#"
        dc.b    "#....##.#..........##.###......................#"
        dc.b    "#..............................................#"
        dc.b    "################################################"
        even

BackCopper:     dc.l    Copper          ; the list being made; the startup starts the first one made, Copper
ShownCopper:    dc.l    Copper2
BackSprites:    dc.l    SpritesA
ShownSprites:   dc.l    SpritesB
OverText:       dc.l    GameOverText
Message:        dc.l    TitleText
SoundNext:      dc.l    0               ; a sound to play (from the game) ...
SoundPlaying:   dc.l    0               ; ... and the one being started (by the interrupt)
SoundLength:    dc.w    0
SoundStep:      dc.w    0
Seed:           dc.l    0
LastFrames:     dc.l    0
Score:          dc.b    0,0,0,0         ; six BCD digits (and a spare byte, so it's cleared as a long)
State:          dc.w    0
TickClock:      dc.w    0
TickCount:      dc.w    0
FrameRate:      dc.w    50
PlayLines:      dc.w    PALPLAY
DiwStop:        dc.w    $2cc1
CamX:           dc.w    0
CamY:           dc.w    0
AttractX:       dc.w    0
AttractY:       dc.w    0
ScrollWord:     dc.w    0
ScrollDelay:    dc.w    0
BufferTop:      dc.w    0
DrawnRow:       dc.w    -1000
DrawnColumn:    dc.w    0
PlayerX:        dc.w    0
PlayerY:        dc.w    0
Lives:          dc.w    0
GemsLeft:       dc.w    0
Safe:           dc.w    0
OverTime:       dc.w    0
JoyX:           dc.w    0
JoyY:           dc.w    0
Enemies:        ds.w    4*ENEMIES       ; each: x, y, x step, y step
Fire:           dc.b    0
FirePressed:    dc.b    0
PanelChanged:   dc.b    0
SoundOff:       dc.b    0
DigitText:      dc.b    "0",0
        even

        section map,bss
MapTiles:       ds.b    MAPW*MAPH

; ---------------------------------------------------------------------------------------------------------------
; What the chips read, in chip RAM.

        section chipdata,data_c

Graphics:       incbin  "ch17_graphics.raw"
        even
NoSprite:       dc.l    0
Silence:        dc.w    0

        section chipbss,bss_c

Copper:         ds.b    1024            ; the two copper lists: one shown, one being made
Copper2:        ds.b    1024
SpritesA:       ds.b    SPRBYTES*4      ; and the sprites each list points at
SpritesB:       ds.b    SPRBYTES*4
Panel:          ds.b    PANELROW*PANELLINES
Buffer:         ds.b    BUFBYTES
