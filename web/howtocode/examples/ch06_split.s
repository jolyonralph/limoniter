; ch06_split.s - one screen in two modes. The top is low resolution, four colours, and wobbles: the copper sets
; a different horizontal scroll (BPLCON1) on every line. Below a white line, the copper switches to high
; resolution, with its own bitplanes, fetch and colours, and moves that part's bitplane pointers each frame to
; scroll it up and down. Left mouse button to leave.
;
; How To Code v8, chapter 6. CC0: use it for anything.
;
; $VER: ch06_split 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

TOPLINE         equ     $2c             ; the display's first line
SPLITLINE       equ     $a0             ; the white line between the two parts (and the line after it)
TOPROWS         equ     SPLITLINE-TOPLINE
TOPWIDTH        equ     352             ; the top picture: 320 shown, and 32 more to scroll into
TOPBYTES        equ     TOPWIDTH/8
BOTWIDTH        equ     640             ; the bottom picture, high resolution
BOTBYTES        equ     BOTWIDTH/8
BOTROWS         equ     200             ; taller than the 138 lines shown, so it can scroll
BOTSHOWN        equ     $12c-SPLITLINE-2
BOTSCROLL       equ     BOTROWS-BOTSHOWN        ; how far it can scroll

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: the pictures, and the top part's bitplanes into the list.

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2        ; NTSC: 200 lines
.pal:
        move.l  #TopPlane1,d0
        move.w  d0,CopperTopBitplanes+6
        swap    d0
        move.w  d0,CopperTopBitplanes+2
        move.l  #TopPlane2,d0
        move.w  d0,CopperTopBitplanes+14
        swap    d0
        move.w  d0,CopperTopBitplanes+10

        ; The top: a chequerboard of 16-pixel squares in plane 1, and of 32-pixel squares (moved down 8 rows)
        ; in plane 2, making four colours.
        lea     TopPlane1,a0
        lea     TopPlane2,a1
        moveq   #0,d7                   ; the row
.toprow:
        moveq   #TOPBYTES/2-1,d6
        move.w  #$ffff,d0
        btst    #4,d7
        beq.s   .p1
        not.w   d0
.p1:
        move.w  d7,d2
        addq.w  #8,d2
        move.l  #$ffff0000,d1
        btst    #5,d2
        beq.s   .p2
        not.l   d1
.p2:
        moveq   #0,d3                   ; the word along the row
.topword:
        move.w  d0,(a0)+
        not.w   d0                      ; plane 1 changes every word (16 pixels)
        btst    #0,d3
        beq.s   .samepair
        swap    d1                      ; plane 2 every two words
.samepair:
        move.w  d1,(a1)+
        addq.w  #1,d3
        dbf     d6,.topword
        addq.w  #1,d7
        cmp.w   #TOPROWS,d7
        blo.s   .toprow

        ; The bottom: rings in plane 1, and stripes 32 pixels apart in plane 2.
        lea     BotPlane1,a0
        bsr     DrawRings
        lea     BotPlane2,a0
        move.w  #BOTROWS-1,d7
.botrow:
        moveq   #BOTBYTES/4-1,d6
.botlong:
        move.l  #$ffff0000,(a0)+
        dbf     d6,.botlong
        dbf     d7,.botrow
        rts

; Rings round the middle of the bottom picture, in the plane at a0: a pixel is set where the square of its distance
; from the middle, divided by 512, is odd. A high resolution pixel is half as wide as a line is tall, so x counts
; half as much. The square is kept up to date by adding as x moves on: (x+1)^2 = x^2 + 2x + 1.
DrawRings:
        moveq   #0,d7                   ; the row
.row:
        move.w  d7,d2
        sub.w   #BOTROWS/2,d2
        muls    d2,d2                   ; dy squared
        move.w  #-BOTWIDTH/2,d1         ; dx
        move.l  #(BOTWIDTH/2)*(BOTWIDTH/2),d0   ; dx squared
        moveq   #BOTBYTES/2-1,d6
.word:
        moveq   #16-1,d5
.pixel:
        move.l  d0,d4
        lsr.l   #2,d4                   ; dx squared / 4: x counts half
        add.l   d2,d4
        add.w   d3,d3
        btst    #9,d4
        beq.s   .clear
        addq.w  #1,d3
.clear:
        move.w  d1,d4
        ext.l   d4
        add.l   d4,d0
        add.l   d4,d0
        addq.l  #1,d0
        addq.w  #1,d1
        dbf     d5,.pixel
        move.w  d3,(a0)+
        dbf     d6,.word
        addq.w  #1,d7
        cmp.w   #BOTROWS,d7
        blo.s   .row
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame.

Frame:
        addq.w  #1,Phase
        lea     Sine,a2

        ; The wobble: a scroll of 0 to 15 pixels for each line of the top, along a sine wave that moves.
        lea     CopperWobble+6,a0       ; the first line's BPLCON1 value
        move.w  Phase,d1
        add.w   d1,d1
        moveq   #TOPROWS-1,d7
.line:
        move.w  d1,d0
        and.w   #255,d0
        add.w   d0,d0
        move.w  (a2,d0.w),d0
        muls    #15,d0
        swap    d0                      ; / 65536: -4 to 3
        addq.w  #8,d0                   ; 4 to 11: a gentle wave around the middle
        move.w  d0,d2
        lsl.w   #4,d2
        or.w    d2,d0                   ; the same scroll for both playfields' bits
        move.w  d0,(a0)
        addq.l  #8,a0
        addq.w  #5,d1                   ; the next line, further along the wave
        dbf     d7,.line

        ; The bottom part's bitplane pointers, in the list after the split: a row further down the picture
        ; scrolls it up.
        move.w  Phase,d0
        and.w   #255,d0
        add.w   d0,d0
        move.w  (a2,d0.w),d0
        muls    #BOTSCROLL/2,d0
        add.l   d0,d0
        add.l   d0,d0
        swap    d0
        add.w   #BOTSCROLL/2,d0         ; 0 to BOTSCROLL rows down
        mulu    #BOTBYTES,d0
        move.l  #BotPlane1,d1
        add.l   d0,d1
        move.w  d1,CopperBotBitplanes+6
        swap    d1
        move.w  d1,CopperBotBitplanes+2
        move.l  #BotPlane2,d1
        add.l   d0,d1
        move.w  d1,CopperBotBitplanes+14
        swap    d1
        move.w  d1,CopperBotBitplanes+10
        rts

VBlank:
        rts

        section data,data

Phase:          dc.w    0

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
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1

        ; The top: low resolution, two bitplanes, fetching 16 pixels early (DDFSTRT $30, not $38) so there's a
        ; word to scroll in from the left. It fetches 42 bytes of each 44-byte row, so the modulo is 2.
        dc.w    DDFSTRT,$0030
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$2200
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,TOPBYTES-42
        dc.w    BPL2MOD,TOPBYTES-42
        dc.w    COLOR00,$0310
        dc.w    COLOR01,$0c52
        dc.w    COLOR02,$0e93
        dc.w    COLOR03,$0fd7
CopperTopBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0

        ; A scroll for each line of the top, filled in by Frame.
CopperWobble:
LINE    set     TOPLINE
        rept    TOPROWS
        dc.w    (LINE<<8)!$07,$fffe,BPLCON1,$0000
LINE    set     LINE+1
        endr

        ; The split. The white line has no bitplanes at all, which leaves the copper a whole line to change the
        ; fetch, the bitplane pointers and the colours, with nothing being fetched or shown meanwhile.
        dc.w    (SPLITLINE<<8)!$07,$fffe
        dc.w    BPLCON0,$0200
        dc.w    COLOR00,$0fff
        dc.w    BPLCON1,$0000
        dc.w    DDFSTRT,$003c           ; high resolution's fetch
        dc.w    DDFSTOP,$00d4
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR01,$036a
        dc.w    COLOR02,$0124
        dc.w    COLOR03,$08cf
CopperBotBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    ((SPLITLINE+1)<<8)!$07,$fffe
        dc.w    COLOR00,$0888
        dc.w    ((SPLITLINE+2)<<8)!$07,$fffe
        dc.w    COLOR00,$0012
        dc.w    BPLCON0,$a200           ; high resolution, two bitplanes

        dc.w    $ffff,$fffe

        section screens,bss_c

TopPlane1:      ds.b    TOPBYTES*TOPROWS
TopPlane2:      ds.b    TOPBYTES*TOPROWS
BotPlane1:      ds.b    BOTBYTES*BOTROWS
BotPlane2:      ds.b    BOTBYTES*BOTROWS
