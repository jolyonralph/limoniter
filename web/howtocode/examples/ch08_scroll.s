; ch08_scroll.s - smooth horizontal scrolling of a landscape 1024 pixels wide, three times the screen, back and
; forth a pixel at a time: the bitplane pointers move in steps of 16 pixels, and BPLCON1's delay does the pixels in
; between. Left mouse button to leave.
;
; How To Code v8, chapter 8. CC0: use it for anything.
;
; $VER: ch08_scroll 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

LANDWIDTH       equ     1024            ; the landscape
ROWBYTES        equ     (16+LANDWIDTH)/8        ; each row in memory: an empty word on the left, then the land
HEIGHT          equ     256
PLANES          equ     3
PLANESIZE       equ     ROWBYTES*HEIGHT
MAXSCROLL       equ     LANDWIDTH-320   ; the furthest right the screen can go
FETCHBYTES      equ     42              ; 21 words a line: the 20 shown and one more for scrolling

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: draw the landscape.

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2        ; NTSC: 200 lines
.pal:
        bsr     DrawLand
        bsr     VBlank                  ; the pointers for the first frame
        rts

; The land: hills made of two sine waves, in seven bands of colour going down, with a post every 128 pixels.
DrawLand:
        lea     Sine,a2
        moveq   #0,d7                   ; x
.column:
        ; The ground's top line: 150, plus two sines, 3 and 7 waves across the land.
        move.w  d7,d0
        mulu    #3*256,d0
        lsr.l   #8,d0
        lsr.l   #2,d0                   ; x * 3 * 256 / 1024: the angle, for 3 waves
        and.w   #255,d0
        add.w   d0,d0
        move.w  (a2,d0.w),d1
        muls    #40,d1
        move.w  d7,d0
        mulu    #7*256,d0
        lsr.l   #8,d0
        lsr.l   #2,d0
        add.w   #40,d0
        and.w   #255,d0
        add.w   d0,d0
        move.w  (a2,d0.w),d2
        muls    #16,d2
        add.l   d2,d1
        add.l   d1,d1
        add.l   d1,d1
        swap    d1                      ; / 16384
        add.w   #150,d1
        move.w  d1,d6                   ; the top of the ground here

        ; This column's byte in each plane, at the top of the ground, and its bit.
        move.w  d6,d0
        mulu    #ROWBYTES,d0
        move.w  d7,d1
        add.w   #16,d1                  ; past the empty word on the left
        lsr.w   #3,d1
        add.w   d1,d0
        lea     Planes,a0
        add.l   d0,a0                   ; plane 1
        move.l  a0,a1
        add.l   #PLANESIZE,a1           ; plane 2
        move.l  a1,a3
        add.l   #PLANESIZE,a3           ; plane 3
        move.w  d7,d1
        not.w   d1                      ; the bit: 7 - (x and 7)

        ; A post: 4 pixels wide and 40 high, standing on the ground, every 128 pixels. Colour 7: all 3 planes.
        move.w  d7,d0
        and.w   #127,d0
        cmp.w   #4,d0
        bhs.s   .nopost
        move.l  a0,a4
        move.l  a1,a5
        move.l  a3,a6
        moveq   #40-1,d0
.post:
        sub.w   #ROWBYTES,a4            ; a line up
        sub.w   #ROWBYTES,a5
        sub.w   #ROWBYTES,a6
        bset    d1,(a4)
        bset    d1,(a5)
        bset    d1,(a6)
        dbf     d0,.post
.nopost:
        ; The ground, down to the bottom: colour 1 at the top, one more every 10 lines down, up to 6. Each
        ; colour's bits go into the planes: bit 0 in plane 1, bit 1 in plane 2, bit 2 in plane 3.
        moveq   #1,d4                   ; the colour
        moveq   #10,d5                  ; lines left in this band
        move.w  d6,d3                   ; y
.ground:
        btst    #0,d4
        beq.s   .n1
        bset    d1,(a0)
.n1:
        btst    #1,d4
        beq.s   .n2
        bset    d1,(a1)
.n2:
        btst    #2,d4
        beq.s   .n3
        bset    d1,(a3)
.n3:
        add.w   #ROWBYTES,a0
        add.w   #ROWBYTES,a1
        add.w   #ROWBYTES,a3
        subq.w  #1,d5
        bne.s   .sameband
        moveq   #10,d5
        cmp.w   #6,d4
        beq.s   .sameband
        addq.w  #1,d4
.sameband:
        addq.w  #1,d3
        cmp.w   #HEIGHT,d3
        blo.s   .ground

        addq.w  #1,d7
        cmp.w   #LANDWIDTH,d7
        blo     .column
        rts

; ---------------------------------------------------------------------------------------------------------------
; Nothing to do in the main loop: the scroll is set in VBlank.

Frame:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of each frame: where the screen is, as bitplane pointers and a delay, into the copper list. The
; copper sets them after its WAIT for line 16, so they're in place by then and are used this frame.
;
; With the fetch starting 16 pixels early, the word fetched first is hidden to the left of the window, and with
; no delay the screen shows the pixels from 16 on. A delay of d moves the picture d pixels right. So to show the
; land from pixel x (which is pixel x+16 in memory, after the empty word), fetch from word w = (x+15)/16 with a
; delay of 16*w - x: 0 to 15.

VBlank:
        addq.w  #1,Angle
        move.w  Angle,d0
        and.w   #255,d0
        add.w   d0,d0
        lea     Sine,a0
        move.w  (a0,d0.w),d0
        muls    #MAXSCROLL/2,d0
        add.l   d0,d0
        add.l   d0,d0
        swap    d0
        add.w   #MAXSCROLL/2,d0         ; x: 0 to MAXSCROLL, back and forth
        move.w  d0,d1
        add.w   #15,d1
        lsr.w   #4,d1                   ; w
        move.w  d1,d2
        lsl.w   #4,d2
        sub.w   d0,d2                   ; the delay
        move.w  d2,d0
        lsl.w   #4,d0
        or.w    d0,d2                   ; the same for both playfields' bits
        move.w  d2,CopperScroll+2
        add.w   d1,d1                   ; w words: 2w bytes
        ext.l   d1
        add.l   #Planes,d1
        lea     CopperBitplanes,a0
        moveq   #PLANES-1,d0
.plane:
        move.w  d1,6(a0)
        swap    d1
        move.w  d1,2(a0)
        swap    d1
        add.l   #PLANESIZE,d1
        addq.l  #8,a0
        dbf     d0,.plane
        rts

        section data,data

Angle:          dc.w    0

; Sine, 256 steps to a full turn, from -16384 to 16384. Made by this line of Python (or with Limoniter's
; Tools > General > Table generator):
;   [round(16384*math.sin(i*math.pi/128)) for i in range(256)]
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

; ---------------------------------------------------------------------------------------------------------------

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81           ; the window: 320 by 256, as usual
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0030           ; the fetch starts 16 pixels early: $38 - 8
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$3200           ; three bitplanes
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,ROWBYTES-FETCHBYTES     ; skip the rest of the row after the 42 bytes fetched
        dc.w    BPL2MOD,ROWBYTES-FETCHBYTES
        dc.w    COLOR00,$0025           ; the sky's top colour, for the border above the window
        dc.w    COLOR01,$0583           ; grass, then deeper and darker
        dc.w    COLOR02,$0472
        dc.w    COLOR03,$0462
        dc.w    COLOR04,$0452
        dc.w    COLOR05,$0442
        dc.w    COLOR06,$0331
        dc.w    COLOR07,$0fe8           ; the posts
        dc.w    $1007,$fffe             ; line 16: VBlank has set what follows by now
CopperScroll:
        dc.w    BPLCON1,$0000
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0

        ; The sky: a band of colour every 16 lines, darker at the top.
LINE    set     $2c
BAND    set     0
        rept    12
        dc.w    (LINE<<8)!$07,$fffe
        dc.w    COLOR00,((BAND/3)<<8)!((2+BAND/2)<<4)!(5+BAND*2/3)
LINE    set     LINE+16
BAND    set     BAND+1
        endr
        dc.w    $ffff,$fffe

        section screen,bss_c

Planes:         ds.b    PLANESIZE*PLANES
