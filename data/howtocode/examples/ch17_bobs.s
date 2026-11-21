; ch17_bobs.s - bobs the way a game draws them: each one first saves the background it's about to cover, and is
; taken off again by putting that back, in the opposite order to the drawing, so bobs that overlap leave nothing
; behind. Eight balls bounce over a wall of tiles, into a screen that isn't shown (double buffering). Unlike
; chapter 7's bobs, there's no spare clean copy of the background: that saves a whole screen of chip RAM, and it
; works over a background that changes, like a scrolling one. Left mouse button to leave.
;
; How To Code v8, chapter 17. CC0: use it for anything. The tiles and the ball are made by ch17_make_graphics.py.
;
; $VER: ch17_bobs 8.0 (21.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
PLANES          equ     4
ROWBYTES        equ     WIDTH/8
LINEBYTES       equ     ROWBYTES*PLANES ; interleaved: a line of each bitplane in turn
SCREENBYTES     equ     LINEBYTES*HEIGHT

BOBS            equ     8
BOBWIDTH        equ     32
BOBHEIGHT       equ     32
BLITWORDS       equ     BOBWIDTH/16+1   ; one more word, for the bits shifted out of the last
BOBSIZE         equ     ((BOBHEIGHT*PLANES)<<6)!BLITWORDS
SAVEBYTES       equ     BLITWORDS*2*PLANES*BOBHEIGHT ; what one bob covers, saved
SCREENMOD       equ     LINEBYTES/PLANES-BLITWORDS*2 ; from the end of a blit's row to the next row
XMAX            equ     WIDTH-BLITWORDS*16
YMAX            equ     HEIGHT-BOBHEIGHT

GR_PALETTE      equ     0               ; in ch17_graphics.raw
GR_TILES        equ     32
GR_BOB          equ     752
GR_MASK         equ     1264

        section code,code

Init:
        move.l  #NoSprite,d0
        lea     CopperSprites,a0
        moveq   #8-1,d1
.sprite:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        dbf     d1,.sprite
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2 ; NTSC: 200 lines
.pal:
        ; The colours, from the graphics file.
        lea     Graphics+GR_PALETTE,a0
        lea     CopperColours,a1
        move.w  #COLOR00,d0
        moveq   #16-1,d1
.colour:
        move.w  d0,(a1)+
        move.w  (a0)+,(a1)+
        addq.w  #2,d0
        dbf     d1,.colour

        ; Both screens get the same wall of tiles: floor, with walls round the edge and a few blocks.
        BLIT_BEGIN
        move.l  #ScreenA,a2
        bsr     DrawWall
        move.l  #ScreenB,a2
        bsr     DrawWall
        BLIT_END

        move.l  #ScreenA,d0
        bra     ShowScreen

; The tiles: wall where the pattern below has a 1, floor elsewhere, a gem where it has a 2.
DrawWall:
        jsr     WaitBlitter
        move.l  #$09f00000,BLTCON0(a5)  ; A to D
        move.l  #$ffffffff,BLTAFWM(a5)
        move.w  #0,BLTAMOD(a5)
        move.w  #ROWBYTES-2,BLTDMOD(a5) ; a tile's word, then on to the next line of the same bitplane
        lea     Pattern(pc),a3
        moveq   #0,d7                   ; the row
.row:
        moveq   #0,d6                   ; the column
.tile:
        moveq   #0,d0
        move.b  (a3)+,d0
        sub.b   #'0',d0
        beq.s   .floor
        cmp.b   #2,d0
        beq.s   .gem
        moveq   #1,d0                   ; the wall's tile
        bra.s   .floor
.gem:
        moveq   #2,d0
.floor:
        lsl.w   #7,d0
        lea     Graphics+GR_TILES,a0
        add.w   d0,a0
        move.w  d7,d0
        mulu    #16*LINEBYTES,d0
        move.w  d6,d1
        add.w   d1,d1
        add.w   d1,d0
        lea     (a2,d0.l),a1
        jsr     WaitBlitter
        move.l  a0,BLTAPTH(a5)
        move.l  a1,BLTDPTH(a5)
        move.w  #((16*PLANES)<<6)!1,BLTSIZE(a5)
        addq.w  #1,d6
        cmp.w   #20,d6
        blo.s   .tile
        addq.w  #1,d7
        cmp.w   #16,d7
        blo.s   .row
        rts

Pattern:
        dc.b    "11111111111111111111"
        dc.b    "10000000000000000001"
        dc.b    "10200000000000000201"
        dc.b    "10001100000000110001"
        dc.b    "10001100002000110001"
        dc.b    "10000000000000000001"
        dc.b    "10000001111110000001"
        dc.b    "10020000000000002001"
        dc.b    "10000000000000000001"
        dc.b    "10000001111110000001"
        dc.b    "10000000000000000001"
        dc.b    "10001100002000110001"
        dc.b    "10001100000000110001"
        dc.b    "10200000000000000201"
        dc.b    "10000000000000000001"
        dc.b    "11111111111111111111"
        even

; ---------------------------------------------------------------------------------------------------------------
; Once a frame, in the screen that isn't shown: take off its bobs from two frames ago, last drawn first; move the
; bobs; then save what each will cover, and draw it. Then show that screen.

Frame:
        BLIT_BEGIN

        ; 1. Off: each saved background back where it came from, in reverse order. Where two bobs overlap, the
        ; second one's save has part of the first in it, so putting the second's back first, then the first's,
        ; leaves the background as it was. The other way round would leave a piece of the first bob behind.
        move.l  DrawList,a2
        move.w  (a2)+,d7                ; how many were drawn
        beq.s   .clean
        move.w  d7,d0
        lsl.w   #2,d0
        add.w   d0,a2                   ; past the last
        lea     Saves,a3
        cmp.l   #ListA,DrawList
        beq.s   .savesa
        lea     SAVEBYTES*BOBS(a3),a3
.savesa:
        move.w  d7,d0
        mulu    #SAVEBYTES,d0
        add.l   d0,a3                   ; past the last save
        subq.w  #1,d7
        jsr     WaitBlitter
        move.l  #$09f00000,BLTCON0(a5)  ; A to D
        move.l  #$ffffffff,BLTAFWM(a5)
        move.w  #0,BLTAMOD(a5)
        move.w  #SCREENMOD,BLTDMOD(a5)
.restore:
        move.l  -(a2),d0                ; where it was: the list, read backwards
        add.l   DrawScreen,d0
        sub.w   #SAVEBYTES,a3           ; its save, also backwards
        jsr     WaitBlitter
        move.l  a3,BLTAPTH(a5)
        move.l  d0,BLTDPTH(a5)
        move.w  #BOBSIZE,BLTSIZE(a5)
        dbf     d7,.restore
.clean:

        ; 2. Move them, bouncing off the edges of the screen.
        lea     Bobs,a3
        moveq   #BOBS-1,d7
.move:
        movem.w (a3),d0-d3              ; x, y, x step, y step
        add.w   d2,d0
        bmi.s   .xturn
        cmp.w   #XMAX,d0
        ble.s   .xok
.xturn:
        neg.w   d2
        add.w   d2,d0
        add.w   d2,d0
.xok:
        add.w   d3,d1
        bmi.s   .yturn
        cmp.w   #YMAX,d1
        ble.s   .yok
.yturn:
        neg.w   d3
        add.w   d3,d1
        add.w   d3,d1
.yok:
        movem.w d0-d3,(a3)
        addq.l  #8,a3
        dbf     d7,.move

        ; 3. For each bob: save the background under it, then draw it there.
        lea     Bobs,a3
        move.l  DrawList,a2
        move.w  #BOBS,(a2)+
        lea     Saves,a4
        cmp.l   #ListA+2,a2
        beq.s   .savesa2
        lea     SAVEBYTES*BOBS(a4),a4
.savesa2:
        moveq   #BOBS-1,d7
.bob:
        movem.w (a3),d0-d1
        addq.l  #8,a3
        mulu.w  #LINEBYTES,d1
        move.w  d0,d2
        lsr.w   #4,d2
        add.w   d2,d2
        add.w   d2,d1                   ; from the start of the screen
        move.l  d1,(a2)+                ; noted, to take it off when this screen comes round again
        add.l   DrawScreen,d1

        ; Save: the screen's rectangle (three words by 128 rows) to the save area, A to D.
        jsr     WaitBlitter
        move.l  #$09f00000,BLTCON0(a5)
        move.l  #$ffffffff,BLTAFWM(a5)
        move.w  #SCREENMOD,BLTAMOD(a5)
        move.w  #0,BLTDMOD(a5)
        move.l  d1,BLTAPTH(a5)
        move.l  a4,BLTDPTH(a5)
        move.w  #BOBSIZE,BLTSIZE(a5)
        lea     SAVEBYTES(a4),a4

        ; Draw: cookie-cut, as in chapter 7. A the mask, B the ball, C and D the screen; D = AB + aC.
        and.w   #15,d0
        ror.w   #4,d0                   ; the shift
        move.w  d0,d2
        or.w    #$0fca,d2
        jsr     WaitBlitter
        move.w  d2,BLTCON0(a5)
        move.w  d0,BLTCON1(a5)
        move.l  #$ffff0000,BLTAFWM(a5)  ; the third word of each row is only the bits shifted out
        move.w  #-2,BLTAMOD(a5)         ; so A and B go back a word for the next row (chapter 7)
        move.w  #-2,BLTBMOD(a5)
        move.w  #SCREENMOD,BLTCMOD(a5)
        move.w  #SCREENMOD,BLTDMOD(a5)
        move.l  #Graphics+GR_MASK,BLTAPTH(a5)
        move.l  #Graphics+GR_BOB,BLTBPTH(a5)
        move.l  d1,BLTCPTH(a5)
        move.l  d1,BLTDPTH(a5)
        move.w  #BOBSIZE,BLTSIZE(a5)
        dbf     d7,.bob
        BLIT_END

        ; 4. Show it from the next frame; the next frame draws in the other screen.
        move.l  DrawScreen,d0
        move.l  ShownScreen,DrawScreen
        move.l  d0,ShownScreen
        move.l  DrawList,d1
        move.l  ShownList,DrawList
        move.l  d1,ShownList
        bra     ShowScreen

VBlank:
        rts

ShowScreen:
        lea     CopperBitplanes,a0
        moveq   #PLANES-1,d1
.plane:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        add.l   #ROWBYTES,d0
        addq.l  #8,a0
        dbf     d1,.plane
        rts

        section data,data

DrawScreen:     dc.l    ScreenB
ShownScreen:    dc.l    ScreenA
DrawList:       dc.l    ListB           ; where each screen's bobs are: how many, then each one's place
ShownList:      dc.l    ListA

Bobs:
        dc.w    20,30,2,1
        dc.w    60,150,-1,2
        dc.w    100,40,3,-2
        dc.w    150,100,-2,-1
        dc.w    200,20,1,3
        dc.w    240,180,-3,1
        dc.w    40,90,2,-3
        dc.w    180,60,-1,-2

ListA:          dc.w    0
                ds.l    BOBS
ListB:          dc.w    0
                ds.l    BOBS

        section copper,data_c

Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$4200           ; four bitplanes
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,LINEBYTES-ROWBYTES
        dc.w    BPL2MOD,LINEBYTES-ROWBYTES
CopperColours:
        ds.l    16
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        dc.w    $ffff,$fffe

NoSprite:       dc.l    0

Graphics:       incbin  "ch17_graphics.raw"
                ds.w    1               ; the last row's blit reads a word past the mask

        section screens,bss_c

ScreenA:        ds.b    SCREENBYTES
ScreenB:        ds.b    SCREENBYTES
Saves:          ds.b    SAVEBYTES*BOBS*2 ; each screen's saves
