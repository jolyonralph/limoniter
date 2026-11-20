; ch08_sprites.s - 31 balls from the 8 hardware sprites. Sprites 0 to 5 are each used five times down the screen
; (a 3-colour ball each time); sprites 6 and 7 are attached, to make one ball of 15 colours. They swing on sine
; waves over a one-bitplane grid. The sprite data is double-buffered: the next frame's positions are written into
; one copy while the sprite DMA reads the other. Left mouse button to leave.
;
; How To Code v8, chapter 8. CC0: use it for anything.
;
; $VER: ch08_sprites 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
USES            equ     5               ; how many times each of sprites 0 to 5 is used down the screen
BALLLINES       equ     16
USESIZE         equ     4+BALLLINES*4   ; the two control words, then two data words a line
CHANSIZE        equ     USES*USESIZE+4  ; and two words of 0 to end the sprite
BUFSIZE         equ     8*CHANSIZE
FIRSTBAND       equ     $34             ; the first use's middle line, and the distance down to the next
BANDSTEP        equ     50
BOB             equ     12              ; how far each ball bobs up and down

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2        ; NTSC: 200 lines
.pal:
        ; The grid: a line every 16 pixels across and down.
        lea     Screen,a0
        moveq   #0,d7
.row:
        moveq   #ROWBYTES/2-1,d6
        move.w  #$8000,d0
        move.w  d7,d1
        and.w   #15,d1
        bne.s   .word
        moveq   #-1,d0                  ; every 16th row: a full line
.word:
        move.w  d0,(a0)+
        dbf     d6,.word
        addq.w  #1,d7
        cmp.w   #HEIGHT,d7
        blo.s   .row

        bsr     MakeBalls

        ; The images go into both buffers now; each frame only the control words change.
        lea     SpriteBufferA,a0
        bsr     FillImages
        lea     SpriteBufferB,a0
        bsr     FillImages

        ; Buffer A is shown first, with its sprites all ended (Frame hasn't placed any yet).
        lea     SpriteBufferA,a0
        moveq   #8-1,d0
.empty:
        clr.l   (a0)
        add.w   #CHANSIZE,a0
        dbf     d0,.empty
        move.l  #SpriteBufferA,Front
        move.l  #SpriteBufferB,Back
        bsr     PointSprites
        rts

; The ball images. A 16-by-16 circle; each pixel's brightness, 1 to 15, falls with its distance from a highlight
; up and left of the middle. The 3-colour ball takes it in 3 steps; the attached ball has all 15.
MakeBalls:
        ; First each pixel's brightness, into a table: 0 outside the circle. Distances are measured in half
        ; pixels from the pixel's middle, so the circle is round and centred: its middle is (15,15), radius 16.
        lea     BrightTable,a0
        moveq   #0,d7                   ; y
.brow:
        moveq   #0,d6                   ; x
.bpixel:
        move.w  d6,d0
        add.w   d0,d0
        sub.w   #15,d0
        muls    d0,d0
        move.w  d7,d1
        add.w   d1,d1
        sub.w   #15,d1
        muls    d1,d1
        add.l   d1,d0
        moveq   #0,d2                   ; outside: 0
        cmp.l   #16*16,d0
        bhs.s   .bstore
        ; The distance squared from the highlight at (9,9), in half pixels: up to about 900 at the far edge.
        move.w  d6,d0
        add.w   d0,d0
        sub.w   #9,d0
        muls    d0,d0
        move.w  d7,d1
        add.w   d1,d1
        sub.w   #9,d1
        muls    d1,d1
        add.l   d1,d0
        divu    #40,d0                  ; 0 to about 22
        moveq   #15,d2
        sub.w   d0,d2
        bgt.s   .bstore
        moveq   #1,d2
.bstore:
        move.b  d2,(a0)+
        addq.w  #1,d6
        cmp.w   #16,d6
        blo.s   .bpixel
        addq.w  #1,d7
        cmp.w   #BALLLINES,d7
        blo.s   .brow

        ; Then the sprites' words, a line at a time: a bit for each pixel, from the left.
        lea     BrightTable,a3
        lea     SmallBall,a0
        lea     BigBall6,a1
        lea     BigBall7,a2
.row:
        moveq   #0,d0                   ; the small ball's two words
        moveq   #0,d1
        moveq   #0,d2                   ; sprite 6's two words
        moveq   #0,d3
        moveq   #0,d4                   ; sprite 7's two words
        moveq   #0,d5
        moveq   #16-1,d7
.pixel:
        add.w   d0,d0                   ; every word moves a bit left for the new pixel
        add.w   d1,d1
        add.w   d2,d2
        add.w   d3,d3
        add.w   d4,d4
        add.w   d5,d5
        moveq   #0,d6
        move.b  (a3)+,d6
        beq.s   .next                   ; outside the circle: 0, transparent
        ; The attached ball: brightness 1 to 15 as it is. Bit 0 goes in sprite 6's first word, bit 1 in its
        ; second; bits 2 and 3 in sprite 7's.
        btst    #0,d6
        beq.s   .a1
        addq.w  #1,d2
.a1:
        btst    #1,d6
        beq.s   .a2
        addq.w  #1,d3
.a2:
        btst    #2,d6
        beq.s   .a3
        addq.w  #1,d4
.a3:
        btst    #3,d6
        beq.s   .a4
        addq.w  #1,d5
.a4:
        ; The small ball: 1 to 15 made 1 to 3 (bit 0 in the first word, bit 1 in the second).
        cmp.w   #5,d6
        blo.s   .c1
        cmp.w   #10,d6
        blo.s   .c2
        addq.w  #1,d0
        addq.w  #1,d1
        bra.s   .next
.c2:
        addq.w  #1,d1
        bra.s   .next
.c1:
        addq.w  #1,d0
.next:
        dbf     d7,.pixel
        move.w  d0,(a0)+
        move.w  d1,(a0)+
        move.w  d2,(a1)+
        move.w  d3,(a1)+
        move.w  d4,(a2)+
        move.w  d5,(a2)+
        cmp.l   #BrightTable+16*BALLLINES,a3
        blo.s   .row
        rts

; The images into every use of every sprite in the buffer at a0.
FillImages:
        moveq   #0,d7                   ; the sprite
.sprite:
        lea     SmallBall,a1
        cmp.w   #6,d7
        blo.s   .small
        lea     BigBall6,a1
        beq.s   .small
        lea     BigBall7,a1
.small:
        move.l  a0,a2
        moveq   #USES-1,d6
.use:
        addq.l  #4,a2                   ; past the control words
        move.l  a1,a3
        moveq   #BALLLINES-1,d5
.line:
        move.l  (a3)+,(a2)+
        dbf     d5,.line
        dbf     d6,.use
        clr.l   (a2)                    ; the end
        add.w   #CHANSIZE,a0
        addq.w  #1,d7
        cmp.w   #8,d7
        blo.s   .sprite
        rts

; The copper list's sprite pointers, at the front buffer's sprites.
PointSprites:
        move.l  Front,d0
        lea     CopperSprites,a0
        moveq   #8-1,d1
.sprite:
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        add.l   #CHANSIZE,d0
        addq.l  #8,a0
        dbf     d1,.sprite
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the control words of every use, in the back buffer. VBlank swaps it in when it's done.

Frame:
        tst.b   Ready
        bne     .done                   ; the last one hasn't been shown yet
        addq.w  #1,Phase
        lea     Sine,a2
        move.l  Back,a0
        moveq   #0,d7                   ; the sprite
.sprite:
        move.l  a0,a1
        moveq   #0,d6                   ; the use
.use:
        ; x: across the screen and back, each ball a little behind the one before.
        move.w  Phase,d0
        add.w   d0,d0
        move.w  d7,d1
        mulu    #21,d1
        add.w   d1,d0
        move.w  d6,d1
        mulu    #50,d1
        add.w   d1,d0
        bsr     SineOf
        muls    #144,d0
        add.l   d0,d0
        add.l   d0,d0
        swap    d0
        add.w   #152,d0                 ; 8 to 296
        move.w  d0,d4
        ; y: its band's line, bobbing up and down.
        move.w  Phase,d0
        mulu    #3,d0
        move.w  d7,d1
        mulu    #30,d1
        add.w   d1,d0
        move.w  d6,d1
        mulu    #40,d1
        add.w   d1,d0
        bsr     SineOf
        muls    #BOB,d0
        add.l   d0,d0
        add.l   d0,d0
        swap    d0
        move.w  d6,d1
        mulu    #BANDSTEP,d1
        add.w   d1,d0
        add.w   #FIRSTBAND,d0
        move.w  d0,d5
        moveq   #0,d3                   ; not attached
        bsr     ControlWords
        add.w   #USESIZE,a1
        addq.w  #1,d6
        cmp.w   #USES,d6
        blo.s   .use
        add.w   #CHANSIZE,a0
        addq.w  #1,d7
        cmp.w   #6,d7
        blo     .sprite

        ; The big ball: sprites 6 and 7 at the same place, once each, going round in a circle. Sprite 7 has the
        ; attach bit, which makes the pair one 15-colour sprite.
        move.w  Phase,d0
        bsr     SineOf
        muls    #120,d0
        add.l   d0,d0
        add.l   d0,d0
        swap    d0
        add.w   #152,d0
        move.w  d0,d4                   ; x
        move.w  Phase,d0
        add.w   #64,d0                  ; a quarter turn on: the cosine
        bsr     SineOf
        muls    #100,d0
        add.l   d0,d0
        add.l   d0,d0
        swap    d0
        add.w   #$2c+120,d0
        move.w  d0,d5                   ; y
        move.l  a0,a1
        moveq   #0,d3
        bsr     ControlWords
        clr.l   USESIZE(a1)             ; used once: it ends after the first
        add.w   #CHANSIZE,a1
        moveq   #1,d3                   ; attached
        bsr     ControlWords
        clr.l   USESIZE(a1)

        st      Ready
.done:
        rts

; Sine of angle d0 (256 to a turn) into d0: -16384 to 16384.
SineOf:
        and.w   #255,d0
        add.w   d0,d0
        move.w  (a2,d0.w),d0
        rts

; A use's two control words, at a1: x on the screen in d4 (0 to 319), its top line in d5 (0 to 511), d3 not 0 to
; attach it to the sprite before.
;   SPRxPOS: bits 15-8 the top line's low 8 bits; bits 7-0 the horizontal start's high 8 bits.
;   SPRxCTL: bits 15-8 the line after the bottom's low 8 bits; bit 7 attach; bit 2 the top line's bit 8; bit 1
;            the line after the bottom's bit 8; bit 0 the horizontal start's low bit.
; The horizontal start is in low resolution pixels from the left of the line: $80 more than x lines it up with
; a display window starting at $81 (measured on the emulator: see chapter 8).
ControlWords:
        move.w  d4,d0
        add.w   #$80,d0                 ; the horizontal start
        move.w  d5,d1                   ; the top line
        move.w  d5,d2
        add.w   #BALLLINES,d2           ; the line after the bottom
        move.w  d1,-(sp)
        lsl.w   #8,d1                   ; top line's low 8 bits in 15-8
        move.w  d0,-(sp)
        lsr.w   #1,d0
        move.b  d0,d1                   ; horizontal start's high 8 bits in 7-0
        move.w  d1,(a1)                 ; SPRxPOS
        move.w  (sp)+,d0
        move.w  d2,d1
        lsl.w   #8,d1                   ; bottom's low 8 bits
        and.w   #1,d0
        or.w    d0,d1                   ; bit 0: horizontal start's low bit
        btst    #8,d2
        beq.s   .stoplow
        bset    #1,d1
.stoplow:
        move.w  (sp)+,d0
        btst    #8,d0
        beq.s   .startlow
        bset    #2,d1
.startlow:
        tst.w   d3
        beq.s   .notattached
        bset    #7,d1
.notattached:
        move.w  d1,2(a1)                ; SPRxCTL
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of each frame: if Frame has finished a buffer, show it. The copper list sets the sprite pointers
; after its WAIT for line 16, before the sprite DMA starts on the frame, so this frame shows it.

VBlank:
        tst.b   Ready
        beq.s   .done
        move.l  Front,d0
        move.l  Back,Front
        move.l  d0,Back
        bsr     PointSprites
        sf      Ready
.done:
        rts

        section data,data

Phase:          dc.w    0
Front:          dc.l    0               ; the sprite data being shown
Back:           dc.l    0               ; the sprite data Frame writes
Ready:          dc.b    0               ; set when Back is ready to be shown
        even

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

        section balls,bss

SmallBall:      ds.l    BALLLINES
BigBall6:       ds.l    BALLLINES
BigBall7:       ds.l    BALLLINES
BrightTable:    ds.b    16*BALLLINES

; ---------------------------------------------------------------------------------------------------------------

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches and 16-pixel sprites, as OCS and ECS have
        dc.w    BPLCON3,$0c00           ; AGA: sprites in low resolution (bits 7-6 = 0), as OCS and ECS have
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200           ; one bitplane
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0024           ; sprites in front of both playfields (0 puts playfield 1 in front)
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0112
        dc.w    COLOR01,$0235
        ; The sprites' colours: 17-19 for sprites 0 and 1, 21-23 for 2 and 3, 25-27 for 4 and 5, 29-31 for 6
        ; and 7. Attached, sprites 6 and 7 use all of 17 to 31 (20, 24 and 28 too), so the 15 make one ramp, from
        ; dark purple to white, and each pair of sprites takes three steps of it.
        dc.w    COLOR17,$0315,COLOR18,$0529,COLOR19,$084c
        dc.w    COLOR20,$0a4b
        dc.w    COLOR21,$0c49,COLOR22,$0e57,COLOR23,$0f66
        dc.w    COLOR24,$0f74
        dc.w    COLOR25,$0f92,COLOR26,$0fb2,COLOR27,$0fd4
        dc.w    COLOR28,$0fe6
        dc.w    COLOR29,$0ff8,COLOR30,$0ffb,COLOR31,$0fff
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    $1007,$fffe             ; line 16: VBlank has set the pointers that follow by now
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        dc.w    DMACON,DMAF_SETCLR!DMAF_SPRITE  ; the startup leaves sprite DMA off: the copper switches it on
        dc.w    $ffff,$fffe

        section sprites,bss_c

SpriteBufferA:  ds.b    BUFSIZE
SpriteBufferB:  ds.b    BUFSIZE

        section screen,bss_c

Screen:         ds.b    ROWBYTES*HEIGHT
