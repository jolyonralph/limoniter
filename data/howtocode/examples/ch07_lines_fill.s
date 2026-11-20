; ch07_lines_fill.s - the blitter drawing lines and filling between them: a turning star and a hexagon, each
; filled in a bitplane of its own (where they overlap, the colour is both), and the hexagon outlined in a third
; bitplane with solid lines. Left mouse button to leave.
;
; How To Code v8, chapter 7. CC0: use it for anything.
;
; The routines DrawLine, DrawFillLine and FillArea are meant to be lifted into your own code: the 3D chapter
; builds on them. Their calling conventions are in the comments above each.
;
; $VER: ch07_lines_fill 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
PLANES          equ     3
ROWBYTES        equ     WIDTH/8         ; one bitplane's line
LINEBYTES       equ     ROWBYTES*PLANES ; one line of all three, as the planes are interleaved
SCREENBYTES     equ     LINEBYTES*HEIGHT

; BLTCON1's bits for area mode (from the NDK's hardware/blit.i)
FILL_OR         equ     $08             ; inclusive fill
FILL_XOR        equ     $10             ; exclusive fill
FILL_CARRYIN    equ     $04             ; start each row filling, not empty
BLITREVERSE     equ     $02             ; descending

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; All eight sprites at an empty sprite, so none shows (with the system running, sprite DMA stays on).
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

        ; Both screens cleared (never count on memory being clear), and the first one shown.
        BLIT_BEGIN
        move.l  #ScreenA,a0
        bsr     ClearScreen
        move.l  #ScreenB,a0
        bsr     ClearScreen
        BLIT_END
        move.l  #ScreenA,d0
        bsr     ShowScreen
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: draw the next picture into the screen that isn't shown, then show it.

Frame:
        addq.w  #1,Turn
        move.l  DrawScreen,a0

        BLIT_BEGIN
        ; The blitter clears the screen while the processor works out where the corners go: the two run side by
        ; side. The processor only touches its own tables here, never the screen being cleared.
        bsr     ClearScreen

        move.w  #WIDTH-1,BoxLeft        ; the box around both shapes, made as the corners are worked out
        move.w  #HEIGHT-1,BoxTop
        clr.w   BoxRight
        clr.w   BoxBottom

        lea     Star,a2
        lea     StarPoints,a3
        move.w  Turn,d0
        move.w  #WIDTH/2,d1
        move.w  #HEIGHT/2,d2
        bsr     MakePolygon

        ; The hexagon's centre goes round in an ellipse, and it turns the other way.
        lea     Sine,a4
        move.w  Turn,d3
        add.w   d3,d3                   ; twice as fast as the star turns
        and.w   #255,d3
        add.w   d3,d3
        move.w  d3,d4
        add.w   #64*2,d4                ; cos(a) is sin(a + a quarter turn)
        and.w   #255*2,d4
        move.w  (a4,d4.w),d1
        muls.w  #50,d1
        asr.l   #7,d1                   ; the table is sin * 16384: shift down by 14
        asr.l   #7,d1
        add.w   #WIDTH/2,d1
        move.w  (a4,d3.w),d2
        muls.w  #40,d2
        asr.l   #7,d2
        asr.l   #7,d2
        add.w   #HEIGHT/2,d2
        lea     Hexagon,a2
        lea     HexagonPoints,a3
        move.w  Turn,d0
        neg.w   d0
        bsr     MakePolygon

        ; The star's edges into bitplane 0, the hexagon's into bitplane 1: one dot a row, for filling.
        move.l  DrawScreen,a0
        move.w  #LINEBYTES,d4           ; from a line of a bitplane to the next: past all three, interleaved
        lea     StarPoints,a2
        moveq   #10,d7
        bsr     PolygonEdges
        lea     ROWBYTES(a0),a0
        lea     HexagonPoints,a2
        moveq   #6,d7
        bsr     PolygonEdges

        ; One fill for both bitplanes, and only the box the shapes are in. Interleaved, the box's rows run
        ; plane 0, plane 1, plane 2, plane 0... so it's one blit of three times the height. Bitplane 2 is still
        ; empty, so filling it changes nothing.
        move.w  BoxLeft,d0
        lsr.w   #4,d0                   ; the box's first and last words
        move.w  BoxRight,d1
        lsr.w   #4,d1
        sub.w   d0,d1
        addq.w  #1,d1                   ; how many words wide
        add.w   d0,d0
        move.l  DrawScreen,a0
        add.w   d0,a0
        move.w  BoxTop,d0
        mulu.w  #LINEBYTES,d0
        add.l   d0,a0                   ; a0: the box's top left word
        move.w  d1,d0                   ; width in words
        moveq   #ROWBYTES/2,d2
        sub.w   d0,d2
        add.w   d2,d2                   ; modulo: the rest of each row
        move.w  BoxBottom,d1
        sub.w   BoxTop,d1
        addq.w  #1,d1
        mulu.w  #PLANES,d1              ; height in rows
        moveq   #FILL_XOR,d3
        bsr     FillArea

        ; Then the hexagon's outline into bitplane 2, every pixel of each line.
        move.l  DrawScreen,a0
        lea     ROWBYTES*2(a0),a0
        move.w  #LINEBYTES,d4
        lea     HexagonPoints,a2
        moveq   #6,d7
        bsr     PolygonOutline
        BLIT_END

        ; The picture is finished (BLIT_END waited for the last blit): show it from the next frame, and draw
        ; the next one in the other screen. The copper reads its list at the top of the frame, long before
        ; this point, so it never sees half a pointer.
        move.l  DrawScreen,d0
        move.l  ShownScreen,DrawScreen
        move.l  d0,ShownScreen
        bsr     ShowScreen
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do.

VBlank:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The screen at d0 into the copper list's bitplane pointers.

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

; ---------------------------------------------------------------------------------------------------------------
; Clears the screen at a0 (all three bitplanes, as they're interleaved: one blit of 768 rows). D only, with
; minterm 0. Doesn't wait for it to finish. Call between BLIT_BEGIN and BLIT_END, with a5 = CUSTOM.

ClearScreen:
        jsr     WaitBlitter
        move.l  #$01000000,BLTCON0(a5)  ; BLTCON0 $0100: D only, LF 0; BLTCON1 0
        move.w  #0,BLTDMOD(a5)
        move.l  a0,BLTDPTH(a5)
        move.w  #((HEIGHT*PLANES)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        rts

; ---------------------------------------------------------------------------------------------------------------
; Works out a shape's corners: a2 = the shape (how many corners, then for each its angle (256 to a turn) and
; distance from the centre), a3 = where its corners go (x, y words), d0 = how far it's turned, d1/d2 = its
; centre. Widens BoxLeft/Top/Right/Bottom to take them in.

MakePolygon:
        lea     Sine,a4
        move.w  (a2)+,d7
        subq.w  #1,d7
.corner:
        move.w  (a2)+,d3                ; the corner's angle, turned
        add.w   d0,d3
        and.w   #255,d3
        add.w   d3,d3
        move.w  (a2)+,d4                ; its distance from the centre
        move.w  d3,d5
        add.w   #64*2,d5
        and.w   #255*2,d5
        move.w  (a4,d5.w),d5            ; x = centre + distance * cos(angle)
        muls.w  d4,d5
        asr.l   #7,d5
        asr.l   #7,d5
        add.w   d1,d5
        move.w  (a4,d3.w),d6            ; y = centre + distance * sin(angle)
        muls.w  d4,d6
        asr.l   #7,d6
        asr.l   #7,d6
        add.w   d2,d6
        move.w  d5,(a3)+
        move.w  d6,(a3)+
        cmp.w   BoxLeft,d5
        bge.s   .1
        move.w  d5,BoxLeft
.1:     cmp.w   BoxRight,d5
        ble.s   .2
        move.w  d5,BoxRight
.2:     cmp.w   BoxTop,d6
        bge.s   .3
        move.w  d6,BoxTop
.3:     cmp.w   BoxBottom,d6
        ble.s   .4
        move.w  d6,BoxBottom
.4:     dbf     d7,.corner
        rts

; ---------------------------------------------------------------------------------------------------------------
; A closed shape's edges, for filling (PolygonEdges) or as an outline (PolygonOutline): a2 = its corners (x, y
; words), d7 = how many, a0 = the bitplane, d4 = bytes from one line of it to the next, a5 = CUSTOM.

PolygonEdges:
        lea     DrawFillLine(pc),a4
        bra.s   Polygon
PolygonOutline:
        lea     DrawLine(pc),a4
Polygon:
        move.l  a2,a3                   ; the first corner, to end on
        subq.w  #1,d7
.edge:
        move.w  d7,-(sp)
        movem.w (a2),d0-d1              ; this corner
        addq.l  #4,a2
        tst.w   (sp)
        bne.s   .next
        move.l  a3,a2                   ; the last edge goes back to the first corner
.next:
        movem.w (a2),d2-d3              ; the next
        jsr     (a4)
        move.w  (sp)+,d7
        dbf     d7,.edge
        rts

; ---------------------------------------------------------------------------------------------------------------
; The line routines. Both draw from (d0,d1) to (d2,d3) into the bitplane at a0, which is d4 bytes from one line
; to the next (40 for a 320-pixel bitplane; more if the bitplanes are interleaved). a5 = CUSTOM. They don't clip:
; both ends must be on the bitplane. They change d0-d3, d5-d7 and a1, and keep d4, a0 and a2-a6.
;
; Each waits for the blitter before setting it up, and starts the line without waiting for it to finish, so
; call them between BLIT_BEGIN and BLIT_END.
;
; DrawLine: a solid line, every pixel set (minterm $CA, as the HRM gives).
;
; DrawFillLine: a line for the area fill to fill between. It has one dot on each line of the screen it crosses
; (BLTCON1's SING bit), and it's drawn with exclusive-or (minterm $4A), so where two edges put a dot in the same
; place they cancel out. Its top end is left off - the line covers the screen lines below its top end down to its
; bottom end, never both ends - so where one edge ends and the next begins there's exactly one dot, not two. A
; level line covers no screen lines at all, and isn't drawn. All of which makes the fill come out right.

DrawFillLine:
        cmp.w   d1,d3
        beq.s   .level
        bgt.s   .down
        exg     d0,d2                   ; drawn from the top down, always
        exg     d1,d3
.down:
        move.w  #$0b4a,d5               ; A, C and D; LF $4A: D = ABc + aC, with B all ones D = A xor C
        moveq   #3,d6                   ; BLTCON1: LINE and SING
        bra.s   LineCommon
.level:
        rts

DrawLine:
        move.w  #$0bca,d5               ; A, C and D; LF $CA: D = AB + aC, with B all ones the dot is set
        moveq   #1,d6                   ; BLTCON1: LINE

LineCommon:
        sub.w   d0,d2                   ; dx
        sub.w   d1,d3                   ; dy
        ; The octant: which way the line goes, as three bits of BLTCON1 (SUD, SUL and AUL). Index the table
        ; by: 1 if dx < 0, + 2 if dy < 0, + 4 if the line is more up and down than across. Then d2 is the
        ; longer of |dx| and |dy|, d3 the shorter.
        moveq   #0,d7
        tst.w   d2
        bpl.s   .dxplus
        neg.w   d2
        addq.w  #1,d7
.dxplus:
        tst.w   d3
        bpl.s   .dyplus
        neg.w   d3
        addq.w  #2,d7
.dyplus:
        cmp.w   d3,d2
        bhs.s   .across
        exg     d2,d3
        addq.w  #4,d7
.across:
        lea     Octants(pc),a1
        or.b    (a1,d7.w),d6

        ; The word with the first pixel in it, and that pixel's place in the word (BLTCON0's top four bits).
        mulu.w  d4,d1
        lea     (a0,d1.l),a1
        move.w  d0,d7
        lsr.w   #3,d7
        and.w   #$fffe,d7
        add.w   d7,a1
        and.w   #15,d0
        ror.w   #4,d0
        or.w    d0,d5

        ; The slope, as the HRM gives it: BLTBMOD = 4 * short, BLTAPTL = 4 * short - 2 * long (and SIGN set in
        ; BLTCON1 if that's negative), BLTAMOD = 4 * (short - long). The length is long + 1 pixels.
        add.w   d3,d3
        add.w   d3,d3                   ; 4 * short
        move.w  d3,d7
        sub.w   d2,d7
        sub.w   d2,d7                   ; 4 * short - 2 * long
        bpl.s   .plus
        or.w    #$40,d6                 ; SIGN
.plus:
        move.w  d7,d1
        sub.w   d2,d1
        sub.w   d2,d1                   ; 4 * (short - long)
        addq.w  #1,d2
        lsl.w   #6,d2
        addq.w  #2,d2                   ; BLTSIZE: the length in pixels as the height; the width is always 2

        jsr     WaitBlitter
        move.w  d5,BLTCON0(a5)          ; BLTCON0 before BLTADAT: the data is shifted as it's written
        move.w  d6,BLTCON1(a5)
        move.w  #$8000,BLTADAT(a5)      ; the one dot, moved along by the blitter
        move.w  #$ffff,BLTBDAT(a5)      ; the pattern: solid
        move.l  #$ffffffff,BLTAFWM(a5)  ; both masks all ones
        move.w  d4,BLTCMOD(a5)
        move.w  d4,BLTDMOD(a5)
        move.w  d3,BLTBMOD(a5)
        move.w  d1,BLTAMOD(a5)
        move.w  d7,BLTAPTL(a5)
        move.l  a1,BLTCPTH(a5)
        ; The blitter writes the line's first dot through BLTDPT and every dot after it through BLTCPT, so a
        ; fill line's first dot goes to a word nobody looks at: that's how its top end is left off.
        btst    #1,d6
        beq.s   .solid
        move.l  #LineDummy,BLTDPTH(a5)
        bra.s   .go
.solid:
        move.l  a1,BLTDPTH(a5)
.go:
        move.w  d2,BLTSIZE(a5)          ; last: this starts it
        rts

; BLTCON1's octant bits, by the index worked out above (the HRM's Table 6-3 octant in brackets).
Octants:
        dc.b    $10                     ; across, right, down (7)
        dc.b    $14                     ; across, left, down (4)
        dc.b    $18                     ; across, right, up (0)
        dc.b    $1c                     ; across, left, up (3)
        dc.b    $00                     ; up and down, right, down (6)
        dc.b    $08                     ; up and down, left, down (5)
        dc.b    $04                     ; up and down, right, up (1)
        dc.b    $0c                     ; up and down, left, up (2)

; ---------------------------------------------------------------------------------------------------------------
; FillArea: fills between the dots in a rectangle, with A to D copying it onto itself in descending mode.
;   a0 = the rectangle's top left word, d0 = its width in words (1 to 64), d1 = its height in rows (1 to 1024),
;   d2 = its modulo (the bytes from the end of one row to the start of the next), d3 = FILL_XOR or FILL_OR
;   (and FILL_CARRYIN if wanted), a5 = CUSTOM.
; Changes d1, d3 and d5-d7 and a1. Waits for the blitter before, not after: call between BLIT_BEGIN and BLIT_END.

FillArea:
        ; Descending mode goes backwards through memory: it starts at the rectangle's last word, bottom right.
        move.w  d0,d5
        add.w   d5,d5                   ; a row's bytes
        move.w  d5,d6
        add.w   d2,d6                   ; from one row to the next
        move.w  d1,d7
        subq.w  #1,d7
        mulu.w  d6,d7
        lea     -2(a0,d7.l),a1
        add.w   d5,a1                   ; the last word
        lsl.w   #6,d1
        move.w  d0,d5
        and.w   #63,d5                  ; 64 words wide is 0
        or.w    d5,d1
        or.w    #BLITREVERSE,d3

        jsr     WaitBlitter
        move.w  #$09f0,BLTCON0(a5)      ; A and D, LF $F0: D = A
        move.w  d3,BLTCON1(a5)
        move.l  #$ffffffff,BLTAFWM(a5)
        move.w  d2,BLTAMOD(a5)
        move.w  d2,BLTDMOD(a5)
        move.l  a1,BLTAPTH(a5)
        move.l  a1,BLTDPTH(a5)
        move.w  d1,BLTSIZE(a5)
        rts

; ---------------------------------------------------------------------------------------------------------------

        section data,data

DrawScreen:     dc.l    ScreenB
ShownScreen:    dc.l    ScreenA
Turn:           dc.w    0
BoxLeft:        dc.w    0
BoxTop:         dc.w    0
BoxRight:       dc.w    0
BoxBottom:      dc.w    0

; The shapes: how many corners, then each corner's angle (256 to a turn) and distance from the centre.
Star:
        dc.w    10
        dc.w    0,100, 26,42, 51,100, 77,42, 102,100, 128,42, 154,100, 179,42, 205,100, 230,42
Hexagon:
        dc.w    6
        dc.w    0,64, 43,64, 85,64, 128,64, 171,64, 213,64

; sin(a) * 16384 for a = 0 to 255 (256 to a turn), made with a Python one-liner (Limoniter's Table generator,
; in Tools, makes tables like this too).
Sine:
        dc.w    0,402,804,1205,1606,2006,2404,2801,3196,3590,3981,4370,4756,5139,5520,5897
        dc.w    6270,6639,7005,7366,7723,8076,8423,8765,9102,9434,9760,10080,10394,10702,11003,11297
        dc.w    11585,11866,12140,12406,12665,12916,13160,13395,13623,13842,14053,14256,14449,14635,14811,14978
        dc.w    15137,15286,15426,15557,15679,15791,15893,15986,16069,16143,16207,16261,16305,16340,16364,16379
        dc.w    16384,16379,16364,16340,16305,16261,16207,16143,16069,15986,15893,15791,15679,15557,15426,15286
        dc.w    15137,14978,14811,14635,14449,14256,14053,13842,13623,13395,13160,12916,12665,12406,12140,11866
        dc.w    11585,11297,11003,10702,10394,10080,9760,9434,9102,8765,8423,8076,7723,7366,7005,6639
        dc.w    6270,5897,5520,5139,4756,4370,3981,3590,3196,2801,2404,2006,1606,1205,804,402
        dc.w    0,-402,-804,-1205,-1606,-2006,-2404,-2801,-3196,-3590,-3981,-4370,-4756,-5139,-5520,-5897
        dc.w    -6270,-6639,-7005,-7366,-7723,-8076,-8423,-8765,-9102,-9434,-9760,-10080,-10394,-10702,-11003,-11297
        dc.w    -11585,-11866,-12140,-12406,-12665,-12916,-13160,-13395,-13623,-13842,-14053,-14256,-14449,-14635,-14811,-14978
        dc.w    -15137,-15286,-15426,-15557,-15679,-15791,-15893,-15986,-16069,-16143,-16207,-16261,-16305,-16340,-16364,-16379
        dc.w    -16384,-16379,-16364,-16340,-16305,-16261,-16207,-16143,-16069,-15986,-15893,-15791,-15679,-15557,-15426,-15286
        dc.w    -15137,-14978,-14811,-14635,-14449,-14256,-14053,-13842,-13623,-13395,-13160,-12916,-12665,-12406,-12140,-11866
        dc.w    -11585,-11297,-11003,-10702,-10394,-10080,-9760,-9434,-9102,-8765,-8423,-8076,-7723,-7366,-7005,-6639
        dc.w    -6270,-5897,-5520,-5139,-4756,-4370,-3981,-3590,-3196,-2801,-2404,-2006,-1606,-1205,-804,-402

        section points,bss

StarPoints:     ds.w    10*2
HexagonPoints:  ds.w    6*2

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the empty sprite, in chip RAM.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$3200           ; three bitplanes, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,LINEBYTES-ROWBYTES ; interleaved: skip the other two bitplanes' lines
        dc.w    BPL2MOD,LINEBYTES-ROWBYTES
        dc.w    COLOR00,$0113           ; the background
        dc.w    COLOR01,$0e62           ; the star
        dc.w    COLOR02,$036c           ; the hexagon
        dc.w    COLOR03,$0b6b           ; both
        dc.w    COLOR04,$0ffd           ; the outline, over any of them
        dc.w    COLOR05,$0ffd
        dc.w    COLOR06,$0ffd
        dc.w    COLOR07,$0ffd
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
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

NoSprite:
        dc.l    0,0

        section screens,bss_c

ScreenA:        ds.b    SCREENBYTES
ScreenB:        ds.b    SCREENBYTES
LineDummy:      ds.w    1               ; where each fill line's first dot goes, unseen
