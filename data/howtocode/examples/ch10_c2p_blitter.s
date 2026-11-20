; ch10_c2p_blitter.s - chunky to planar on an Amiga 500: a rotozoomer in 16 colours, 2x2 pixels, drawn in chunky
; form by the 68000 and turned into 4 bitplanes by the blitter. The texture's colours are stored ready-made for
; the conversion, so the processor's drawing does half of it for nothing; the blitter's two rounds of merges do
; the rest. The panel gives the time each part took, in frames, measured with a line counter, and how many
; frames each new picture took in all. Left mouse button to leave.
;
; How To Code v8, chapter 10. CC0: use it for anything.
;
; Set BLITTER to 0 and the 68000 does the blitter's two rounds itself, to compare.
;
; $VER: ch10_c2p_blitter 8.0 (20.11.2026)

OS_FRIENDLY     equ     0
BLITTER         equ     1

        include "hardware.i"
        include "system.i"
        include "startup.i"

CHUNKYW         equ     128             ; the chunky buffer, in pixels: each is 2 by 2 on the screen
CHUNKYH         equ     80
WIDTH           equ     CHUNKYW*2       ; 256 pixels across
HEIGHT          equ     CHUNKYH*2       ; 160 lines
ROWBYTES        equ     WIDTH/8         ; 32: one bitplane's row
PLANES          equ     4
ROWSTRIDE       equ     ROWBYTES*PLANES ; interleaved, 128 bytes a row (and the chunky buffer's rows are too)
SCREENBYTES     equ     ROWSTRIDE*CHUNKYH
FIRSTLINE       equ     $2c
PANELLINE       equ     FIRSTLINE+HEIGHT+1
PANELHEIGHT     equ     23
LASTLINE        equ     PANELLINE+PANELHEIGHT

; The texture: 256 by 128 texels, as in ch10_c2p_cpu.s: every row twice over, the whole twice over.
TEXBYTES        equ     512*128

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #263,LinesPerFrame
.pal:
        move.l  #Screen1,ShowScreen
        move.l  #Screen2,DrawScreen
        move.l  #Screen1,PendingScreen
        st      SwapPending
        bsr     VBlank

        move.l  #Panel,d0
        lea     CopperPanel,a0
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)

        bsr     MakeCopperDoubling
        bsr     MakeTexture

        ; Clear both screens and the chunky buffers: chip RAM in a BSS section is cleared as the program
        ; loads, but this is cheap and makes sure.
        lea     Screen1,a0
        move.w  #(SCREENBYTES*2+ROWSTRIDE*CHUNKYH*2)/4-1,d0
.clear:
        clr.l   (a0)+
        dbf     d0,.clear

        bsr     WhichProcessor
        moveq   #0,d0
        moveq   #2,d1
        bsr     DrawText
        lea     ModeText,a0
        moveq   #6,d0
        moveq   #2,d1
        bsr     DrawText
        lea     LabelsText,a0
        moveq   #0,d0
        moveq   #12,d1
        bsr     DrawText
        rts

; ---------------------------------------------------------------------------------------------------------------
; As often as it can: draw the next picture, turn it into bitplanes in the screen not on show, show it.

Frame:
.waitswap:
        tst.b   SwapPending             ; (with the system running: see chapter 13)
        beq.s   .swapped
        jsr     WaitFrame
        bra.s   .waitswap
.swapped:
        move.l  Frames,d0
        move.l  d0,d1
        sub.l   LastPicture,d1
        move.l  d0,LastPicture
        move.w  d1,FramesPerPicture

        bsr     LineClock
        move.l  d0,TimeStart
        bsr     Render
        bsr     LineClock
        move.l  d0,d7
        sub.l   TimeStart,d0
        move.l  d0,RenderLines
        move.l  d7,TimeStart

        lea     CUSTOM,a5
        ifne    BLITTER
        bsr     BlitterC2P
        else
        bsr     CPUC2P
        endc

        bsr     LineClock
        sub.l   TimeStart,d0
        move.l  d0,C2PLines

        move.l  DrawScreen,d0
        move.l  ShowScreen,DrawScreen
        move.l  d0,ShowScreen
        move.l  d0,PendingScreen
        st      SwapPending

        bsr     ShowTimes
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame: the finished screen into the copper list (which reads it after line $28).
; The bitplanes are in the order 3, 1, 2, 0 in each row (see BlitterC2P), so plane 0 is the last of the four.

VBlank:
        tst.b   SwapPending
        beq.s   .none
        move.l  PendingScreen,d0
        lea     CopperBitplanes,a0
        lea     PlaneSlots(pc),a1
        moveq   #PLANES-1,d1
.plane:
        move.l  d0,d2
        add.w   (a1)+,d2
        move.w  d2,6(a0)
        swap    d2
        move.w  d2,2(a0)
        addq.l  #8,a0
        dbf     d1,.plane
        sf      SwapPending
.none:
        rts

PlaneSlots:
        dc.w    3*ROWBYTES,1*ROWBYTES,2*ROWBYTES,0*ROWBYTES     ; where bitplanes 0, 1, 2 and 3 are in a row

; ---------------------------------------------------------------------------------------------------------------
; The time, in lines: the frames so far (Frames, counted by the vertical blank interrupt at line 0) times the
; lines in a frame, plus the line the beam is on now. Read again if the interrupt came in between, and wait out
; lines 0 and 1, so that the frame's interrupt has certainly been and gone. (CIA-B's time-of-day counter counts
; lines too, but with the system running graphics.library sets it back every frame.)
; Returns d0. Changes d1-d3.

LineClock:
        move.l  Frames,d1
        move.l  VPOSR+CUSTOM,d0
        cmp.l   Frames,d1
        bne.s   LineClock
        lsr.l   #8,d0
        and.l   #$1ff,d0                ; the line
        cmp.w   #2,d0
        blo.s   LineClock
        moveq   #0,d3
        move.w  LinesPerFrame,d3
        move.l  d1,d2                   ; Frames * lines a frame, in 32 bits
        swap    d2
        mulu    d3,d2
        swap    d2
        clr.w   d2
        mulu    d3,d1
        add.l   d2,d1
        add.l   d1,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; The rotozoomer, as in ch10_c2p_cpu.s, but writing each chunky line in the order the conversion wants.
;
; Each chunky pixel is a byte holding its colour's four bits each doubled (colour %abcd is %aabbccdd): the two
; screen pixels it makes, side by side, already in the bit order of a bitplane. And each line of 128 is written
; as four runs of 32: the pixels 0, 4, 8 ... 124, then 2, 6 ... 126, then 1, 5 ... 125, then 3, 7 ... 127. Both
; cost the drawing nothing (the texture holds the doubled colours; the table of offsets is in that order), and
; both are work the blitter needn't do.

Render:
        move.l  Frames,d0
        lea     Sine,a0
        move.w  d0,d1
        and.w   #255,d1
        add.w   d1,d1
        move.w  0(a0,d1.w),d2           ; sin(angle)
        add.w   #64*2,d1
        and.w   #511,d1
        move.w  0(a0,d1.w),d3           ; cos(angle)
        move.w  d0,d1
        mulu    #3,d1
        lsr.w   #1,d1
        and.w   #255,d1
        add.w   d1,d1
        move.w  0(a0,d1.w),d4
        muls    #192,d4
        asr.l   #8,d4
        asr.l   #6,d4
        add.w   #320,d4                 ; zoom, 8.8: 0.5 to 2 texels a screen pixel...
        add.w   d4,d4                   ; ...and a chunky pixel is two
        muls    d4,d3
        asr.l   #8,d3
        asr.l   #6,d3                   ; du
        muls    d4,d2
        asr.l   #8,d2
        asr.l   #6,d2                   ; dv
        move.w  d3,d6
        move.w  d2,d7
        ext.l   d6
        ext.l   d7

        ; Deltas, in the order the pixels are written: four runs, each every fourth pixel.
        lea     Deltas,a2
        lea     RunStarts(pc),a3
        moveq   #4-1,d5
.run:
        move.w  (a3)+,d0
        sub.w   #CHUNKYW/2,d0           ; x of the run's first pixel, from the middle
        move.w  d0,d1
        muls    d6,d0                   ; u
        muls    d7,d1                   ; v
        move.l  d6,a4
        add.l   a4,a4
        add.l   a4,a4                   ; 4 du
        move.l  d7,a5
        add.l   a5,a5
        add.l   a5,a5                   ; 4 dv
        moveq   #CHUNKYW/4-1,d4
.delta:
        bsr     TexelOffset
        move.w  d2,(a2)+
        add.l   a4,d0
        add.l   a5,d1
        dbf     d4,.delta
        dbf     d5,.run

        move.l  Frames,d0
        move.l  d0,d1
        lsl.l   #8,d0
        add.l   d1,d0
        mulu    #180,d1
        move.w  #CHUNKYH/2,d2
        muls    d7,d2
        add.l   d2,d0
        move.w  #CHUNKYH/2,d2
        muls    d6,d2
        sub.l   d2,d1

        lea     Chunky,a1
        move.w  #CHUNKYH-1,d5
.line:
        bsr     TexelOffset
        lea     Texture+32768,a0
        add.l   d2,a0
        lea     Deltas,a2
        moveq   #CHUNKYW/32-1,d4
.pixels:
        rept    32
        move.w  (a2)+,d3
        move.b  0(a0,d3.w),(a1)+
        endr
        dbf     d4,.pixels
        sub.l   d7,d0
        add.l   d6,d1
        dbf     d5,.line
        rts

RunStarts:
        dc.w    0,2,1,3

; (u, v) in d0 and d1 (8.8) to a texel offset in d2: (u & 255) + (v & 127) * 512. Changes d3.
TexelOffset:
        move.l  d1,d2
        lsr.l   #8,d2
        and.l   #127,d2
        lsl.w   #8,d2
        add.w   d2,d2
        move.l  d0,d3
        lsr.l   #8,d3
        and.w   #255,d3
        or.w    d3,d2
        rts

; ---------------------------------------------------------------------------------------------------------------
; Chunky to planar by the blitter: two rounds of merges, each two blits.
;
; Think of a chunky line in blocks of 8 chunky pixels (16 on the screen). Each block is four words, one in each
; of the four runs: run R's word holds pixels R and R+4 of the block, a byte each. Its 16 bits are two pixels'
; doubled colours; a bitplane's word is 16 pixels of one colour bit. Two rounds of merges between the runs turn
; one into the other (the same transpose as on the AGA, only smaller):
;
;   s = 4, mask $0F0F, between runs 0 and 2, and 1 and 3
;   s = 2, mask $3333, between runs 0 and 1, and 2 and 3
;
; after which run 0 is bitplane 3, run 1 bitplane 2, run 2 bitplane 1 and run 3 bitplane 0.
;
; One merge is two blits, as the blitter makes one result at a time. The low one of each pair keeps its own
; bits where the mask is 0 and takes the high one's, shifted right by s, where it's 1:
;
;   low'  = (high >> s) & mask  |  low & ~mask     A = high, shifted; B = low; C = the mask, a constant: $E4
;
; and the high one keeps its bits where the mask is 1 and takes the low one's, shifted left by s, where it's 0.
; The blitter shifts only to the right, except in descending mode, so that blit runs backwards:
;
;   high' = (low << s) & ~mask  |  high & mask     A = low, shifted; B = high; C = the mask: $D8, descending
;
; Either way the bits shifted in from the next word fall where the mask throws them away. C is switched off
; and BLTCDAT holds the mask: a constant costs nothing. A, B and D on: 6 clocks a word (chapter 7).
;
; The runs are laid out so each blit is one blit: in each chunky line, run 0, run 2, run 1, run 3 (32 bytes
; each), and the same in the halfway buffer; then the first round's pairs are every other run, and the second's
; are side by side. The screen's rows hold the bitplanes in the order 3, 1, 2, 0 for the same reason.
;
; The blitter can only read and write chip RAM: the chunky buffer, the halfway buffer and the screen are all
; there. Each blit waits for the one before (there's only one blitter), and the last is waited for before
; returning: Render writes the chunky buffer next, and the blitter mustn't still be reading it, nor the screen
; be shown before it's finished (BLIT_END waits; chapter 7 explains why it must).

C2PBYTES        equ     ROWSTRIDE*CHUNKYH       ; the chunky buffer's size, and the halfway buffer's
LASTROW         equ     ROWSTRIDE*(CHUNKYH-1)

; ---- blitterc2p begin
BlitterC2P:
        BLIT_BEGIN
        move.l  #$ffffffff,BLTAFWM(a5)  ; no masking at the edges
        moveq   #ROWBYTES,d0
        move.w  d0,BLTAMOD(a5)          ; the first round: 32 bytes of each run, then skip 32
        move.w  d0,BLTBMOD(a5)
        move.w  d0,BLTDMOD(a5)
        move.w  #$0f0f,BLTCDAT(a5)

        ; Round 1, low: runs 0 and 1 take bits from runs 2 and 3.
        move.l  #$4de40000,BLTCON0(a5)  ; A shifted 4, A B D on, $E4; BLTCON1 0
        move.l  #Chunky+ROWBYTES,BLTAPTH(a5)    ; runs 2 and 3
        move.l  #Chunky,BLTBPTH(a5)             ; runs 0 and 1
        move.l  #Halfway,BLTDPTH(a5)
        move.w  #((CHUNKYH*2)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        jsr     WaitBlitter

        ; Round 1, high: runs 2 and 3 take bits from runs 0 and 1. Descending: from the last word of each.
        move.l  #$4dd80002,BLTCON0(a5)  ; A shifted 4 (left, descending), $D8; BLTCON1: DESC
        move.l  #Chunky+LASTROW+ROWBYTES*2+ROWBYTES-2,BLTAPTH(a5)  ; run 1's last word
        move.l  #Chunky+LASTROW+ROWBYTES*3+ROWBYTES-2,BLTBPTH(a5)  ; run 3's
        move.l  #Halfway+LASTROW+ROWBYTES*3+ROWBYTES-2,BLTDPTH(a5)
        move.w  #((CHUNKYH*2)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        jsr     WaitBlitter

        ; Round 2: pairs side by side, 64 bytes of each line, then skip 64.
        moveq   #ROWBYTES*2,d0
        move.w  d0,BLTAMOD(a5)
        move.w  d0,BLTBMOD(a5)
        move.w  d0,BLTDMOD(a5)
        move.w  #$3333,BLTCDAT(a5)
        move.l  DrawScreen,a0

        ; Round 2, low: runs 0 and 2 take bits from 1 and 3, into bitplanes 3 and 1.
        move.l  #$2de40000,BLTCON0(a5)
        move.l  #Halfway+ROWBYTES*2,BLTAPTH(a5) ; runs 1 and 3
        move.l  #Halfway,BLTBPTH(a5)            ; runs 0 and 2
        move.l  a0,BLTDPTH(a5)
        move.w  #(CHUNKYH<<6)!ROWBYTES,BLTSIZE(a5)
        jsr     WaitBlitter

        ; Round 2, high: runs 1 and 3 take bits from 0 and 2, into bitplanes 2 and 0.
        move.l  #$2dd80002,BLTCON0(a5)
        move.l  #Halfway+LASTROW+ROWBYTES*2-2,BLTAPTH(a5)          ; run 2's last word
        move.l  #Halfway+LASTROW+ROWBYTES*4-2,BLTBPTH(a5)          ; run 3's
        lea     LASTROW+ROWBYTES*4-2(a0),a0
        move.l  a0,BLTDPTH(a5)
        move.w  #(CHUNKYH<<6)!ROWBYTES,BLTSIZE(a5)
        BLIT_END
        rts
; ---- blitterc2p end

; ---------------------------------------------------------------------------------------------------------------
; The same two rounds by the 68000, for comparison: two blocks (a longword of each run) at a time, straight from
; the chunky buffer into the screen.

MERGE           macro                   ; \1 = low, \2 = high, \3 = s, \4 = the mask, in a register; d4 changes
        move.l  \2,d4
        lsr.l   #\3,d4
        eor.l   \1,d4
        and.l   \4,d4
        eor.l   d4,\1
        lsl.l   #\3,d4
        eor.l   d4,\2
        endm

; ---- cpuc2p begin
CPUC2P:
        lea     Chunky,a0
        move.l  DrawScreen,a1
        move.l  #$0f0f0f0f,d5
        move.l  #$33333333,d6
        move.w  #CHUNKYH*ROWBYTES/4-1,d7        ; longwords of each run, in all
        moveq   #ROWBYTES/4,d3
        move.w  d3,a2                   ; counts the longwords to the end of a line
.long:
        move.l  (a0),d0                 ; run 0
        move.l  ROWBYTES(a0),d2         ; run 2
        move.l  ROWBYTES*2(a0),d1       ; run 1
        move.l  ROWBYTES*3(a0),d3       ; run 3
        MERGE   d0,d2,4,d5
        MERGE   d1,d3,4,d5
        MERGE   d0,d1,2,d6
        MERGE   d2,d3,2,d6
        move.l  d0,(a1)                 ; bitplane 3
        move.l  d2,ROWBYTES(a1)         ; bitplane 1
        move.l  d1,ROWBYTES*2(a1)       ; bitplane 2
        move.l  d3,ROWBYTES*3(a1)       ; bitplane 0
        addq.l  #4,a0
        addq.l  #4,a1
        subq.w  #1,a2
        move.w  a2,d0
        bne.s   .same
        lea     ROWSTRIDE-ROWBYTES(a0),a0       ; on to the next line
        lea     ROWSTRIDE-ROWBYTES(a1),a1
        move.w  #ROWBYTES/4,a2
.same:
        dbf     d7,.long
        rts
; ---- cpuc2p end

; ---------------------------------------------------------------------------------------------------------------
; Each row of bitplanes shown twice: the copper sets the modulos on every line, -ROWBYTES after the first of a
; pair (back to the row's start), ROWSTRIDE-ROWBYTES after the second (past the other bitplanes to the next).

MakeCopperDoubling:
        lea     CopperLines,a0
        move.w  #FIRSTLINE,d0
        move.w  #HEIGHT-1,d1
.line:
        move.w  d0,d2
        lsl.w   #8,d2
        or.w    #$07,d2
        move.w  d2,(a0)+
        move.w  #$fffe,(a0)+
        move.w  #ROWSTRIDE-ROWBYTES,d2
        btst    #0,d1
        beq.s   .second
        move.w  #-ROWBYTES,d2
.second:
        move.w  #BPL1MOD,(a0)+
        move.w  d2,(a0)+
        move.w  #BPL2MOD,(a0)+
        move.w  d2,(a0)+
        addq.w  #1,d0
        dbf     d1,.line
        rts

; ---------------------------------------------------------------------------------------------------------------
; The texture: (u XOR v * 2) / 16, a colour from 0 to 15, stored with its bits doubled.

MakeTexture:
        lea     Texture,a0
        lea     DoubledBits(pc),a1
        move.w  #256-1,d7
.row:
        move.w  d7,d1
        not.w   d1
        add.w   d1,d1
        move.w  #512-1,d6
        moveq   #0,d0
.texel:
        move.b  d0,d2
        eor.b   d1,d2
        lsr.b   #4,d2
        and.w   #15,d2
        move.b  0(a1,d2.w),(a0)+
        addq.b  #1,d0
        dbf     d6,.texel
        dbf     d7,.row
        rts

DoubledBits:
        dc.b    $00,$03,$0c,$0f,$30,$33,$3c,$3f,$c0,$c3,$cc,$cf,$f0,$f3,$fc,$ff

; ---------------------------------------------------------------------------------------------------------------
; The panel.

ShowTimes:
        move.l  RenderLines,d0
        moveq   #6,d1
        bsr.s   ShowFrames
        move.l  C2PLines,d0
        moveq   #15,d1
        bsr.s   ShowFrames
        moveq   #0,d0
        move.w  FramesPerPicture,d0
        cmp.w   #99,d0
        bls.s   .fits
        moveq   #99,d0
.fits:
        divu    #10,d0
        lea     TimeText,a0
        move.b  #" ",(a0)
        tst.w   d0
        beq.s   .units
        add.b   #"0",d0
        move.b  d0,(a0)
.units:
        swap    d0
        add.b   #"0",d0
        move.b  d0,1(a0)
        clr.b   2(a0)
        moveq   #30,d0
        moveq   #12,d1
        bra     DrawText

; A time in lines (d0) as frames to two places, at column d1 of the times' row. (As in ch10_c2p_cpu.s.)
ShowFrames:
        move.w  d1,-(sp)
        move.w  LinesPerFrame,d1
        mulu    #100,d0
        moveq   #0,d2
        move.w  d1,d2
        lsr.w   #1,d2
        add.l   d2,d0
        divu    d1,d0
        and.l   #$ffff,d0
        cmp.l   #9999,d0
        bls.s   .digits
        move.l  #9999,d0
.digits:
        lea     TimeText+5,a0
        clr.b   (a0)
        moveq   #5-1,d2
.digit:
        cmp.w   #2,d2
        bne.s   .figure
        move.b  #".",-(a0)
        bra.s   .next
.figure:
        divu    #10,d0
        swap    d0
        add.b   #"0",d0
        move.b  d0,-(a0)
        clr.w   d0
        swap    d0
.next:
        dbf     d2,.digit
        cmp.b   #"0",(a0)
        bne.s   .draw
        move.b  #" ",(a0)
.draw:
        move.w  (sp)+,d0
        moveq   #12,d1
        bra     DrawText

WhichProcessor:
        move.l  ExecBase.w,a6
        move.w  AttnFlags(a6),d0
        lea     Name68060,a0
        btst    #AFB_68060,d0
        bne.s   .known
        lea     Name68040,a0
        btst    #AFB_68040,d0
        bne.s   .known
        lea     Name68030,a0
        btst    #AFB_68030,d0
        bne.s   .known
        lea     Name68020,a0
        btst    #AFB_68020,d0
        bne.s   .known
        lea     Name68010,a0
        btst    #AFB_68010,d0
        bne.s   .known
        lea     Name68000,a0
.known:
        rts

; Text into the panel: a0 = the string, d0 = the column (0 to 31), d1 = the row in pixels.
DrawText:
        lea     Panel,a1
        mulu    #ROWBYTES,d1
        add.l   d1,a1
        add.w   d0,a1
.char:
        moveq   #0,d0
        move.b  (a0)+,d0
        beq.s   .done
        sub.b   #32,d0
        and.w   #63,d0
        lsl.w   #3,d0
        lea     Font,a2
        add.w   d0,a2
        move.b  (a2)+,(a1)
        move.b  (a2)+,ROWBYTES*1(a1)
        move.b  (a2)+,ROWBYTES*2(a1)
        move.b  (a2)+,ROWBYTES*3(a1)
        move.b  (a2)+,ROWBYTES*4(a1)
        move.b  (a2)+,ROWBYTES*5(a1)
        move.b  (a2)+,ROWBYTES*6(a1)
        move.b  (a2)+,ROWBYTES*7(a1)
        addq.l  #1,a1
        bra.s   .char
.done:
        rts

        section data,data

LastPicture:    dc.l    0
LinesPerFrame:  dc.w    313             ; PAL: lines 0 to 312 (NTSC: 263, set by Init)
SwapPending:    dc.b    0
        even

Sine:
        dc.w    0,402,804,1205,1606,2006,2404,2801
        dc.w    3196,3590,3981,4370,4756,5139,5520,5897
        dc.w    6270,6639,7005,7366,7723,8076,8423,8765
        dc.w    9102,9434,9760,10080,10394,10702,11003,11297
        dc.w    11585,11866,12140,12406,12665,12916,13160,13395
        dc.w    13623,13842,14053,14256,14449,14635,14811,14978
        dc.w    15137,15286,15426,15557,15679,15791,15893,15986
        dc.w    16069,16143,16207,16261,16305,16340,16364,16379
        dc.w    16383,16379,16364,16340,16305,16261,16207,16143
        dc.w    16069,15986,15893,15791,15679,15557,15426,15286
        dc.w    15137,14978,14811,14635,14449,14256,14053,13842
        dc.w    13623,13395,13160,12916,12665,12406,12140,11866
        dc.w    11585,11297,11003,10702,10394,10080,9760,9434
        dc.w    9102,8765,8423,8076,7723,7366,7005,6639
        dc.w    6270,5897,5520,5139,4756,4370,3981,3590
        dc.w    3196,2801,2404,2006,1606,1205,804,402
        dc.w    0,-402,-804,-1205,-1606,-2006,-2404,-2801
        dc.w    -3196,-3590,-3981,-4370,-4756,-5139,-5520,-5897
        dc.w    -6270,-6639,-7005,-7366,-7723,-8076,-8423,-8765
        dc.w    -9102,-9434,-9760,-10080,-10394,-10702,-11003,-11297
        dc.w    -11585,-11866,-12140,-12406,-12665,-12916,-13160,-13395
        dc.w    -13623,-13842,-14053,-14256,-14449,-14635,-14811,-14978
        dc.w    -15137,-15286,-15426,-15557,-15679,-15791,-15893,-15986
        dc.w    -16069,-16143,-16207,-16261,-16305,-16340,-16364,-16379
        dc.w    -16384,-16379,-16364,-16340,-16305,-16261,-16207,-16143
        dc.w    -16069,-15986,-15893,-15791,-15679,-15557,-15426,-15286
        dc.w    -15137,-14978,-14811,-14635,-14449,-14256,-14053,-13842
        dc.w    -13623,-13395,-13160,-12916,-12665,-12406,-12140,-11866
        dc.w    -11585,-11297,-11003,-10702,-10394,-10080,-9760,-9434
        dc.w    -9102,-8765,-8423,-8076,-7723,-7366,-7005,-6639
        dc.w    -6270,-5897,-5520,-5139,-4756,-4370,-3981,-3590
        dc.w    -3196,-2801,-2404,-2006,-1606,-1205,-804,-402

Name68000:      dc.b    "68000",0
Name68010:      dc.b    "68010",0
Name68020:      dc.b    "68020",0
Name68030:      dc.b    "68030",0
Name68040:      dc.b    "68040",0
Name68060:      dc.b    "68060",0
        ifne    BLITTER
ModeText:       dc.b    "BLITTER C2P, 4 PLANES",0
        else
ModeText:       dc.b    "CPU C2P, 4 PLANES",0
        endc
LabelsText:     dc.b    "RENDER      C2P      FR EVERY",0
        even

; chapter 14's 8 by 8 font, from the space to the underline
Font:
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; space
        dc.b    $10,$10,$10,$10,$10,$00,$10,$00   ; !
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; "
        dc.b    $28,$28,$7c,$28,$7c,$28,$28,$00   ; #
        dc.b    $10,$3c,$50,$38,$14,$78,$10,$00   ; $
        dc.b    $60,$64,$08,$10,$20,$4c,$0c,$00   ; %
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; &
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; '
        dc.b    $08,$10,$20,$20,$20,$10,$08,$00   ; (
        dc.b    $20,$10,$08,$08,$08,$10,$20,$00   ; )
        dc.b    $00,$10,$54,$38,$54,$10,$00,$00   ; *
        dc.b    $00,$10,$10,$7c,$10,$10,$00,$00   ; +
        dc.b    $00,$00,$00,$00,$18,$08,$10,$00   ; ,
        dc.b    $00,$00,$00,$7c,$00,$00,$00,$00   ; -
        dc.b    $00,$00,$00,$00,$00,$30,$30,$00   ; .
        dc.b    $00,$04,$08,$10,$20,$40,$00,$00   ; /
        dc.b    $38,$44,$4c,$54,$64,$44,$38,$00   ; 0
        dc.b    $10,$30,$10,$10,$10,$10,$38,$00   ; 1
        dc.b    $38,$44,$04,$08,$10,$20,$7c,$00   ; 2
        dc.b    $7c,$08,$10,$08,$04,$44,$38,$00   ; 3
        dc.b    $08,$18,$28,$48,$7c,$08,$08,$00   ; 4
        dc.b    $7c,$40,$78,$04,$04,$44,$38,$00   ; 5
        dc.b    $18,$20,$40,$78,$44,$44,$38,$00   ; 6
        dc.b    $7c,$04,$08,$10,$20,$20,$20,$00   ; 7
        dc.b    $38,$44,$44,$38,$44,$44,$38,$00   ; 8
        dc.b    $38,$44,$44,$3c,$04,$08,$30,$00   ; 9
        dc.b    $00,$30,$30,$00,$30,$30,$00,$00   ; :
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ;
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; <
        dc.b    $00,$00,$7c,$00,$7c,$00,$00,$00   ; =
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; >
        dc.b    $38,$44,$04,$08,$10,$00,$10,$00   ; ?
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; @
        dc.b    $38,$44,$44,$7c,$44,$44,$44,$00   ; A
        dc.b    $78,$44,$44,$78,$44,$44,$78,$00   ; B
        dc.b    $38,$44,$40,$40,$40,$44,$38,$00   ; C
        dc.b    $70,$48,$44,$44,$44,$48,$70,$00   ; D
        dc.b    $7c,$40,$40,$78,$40,$40,$7c,$00   ; E
        dc.b    $7c,$40,$40,$78,$40,$40,$40,$00   ; F
        dc.b    $38,$44,$40,$5c,$44,$44,$3c,$00   ; G
        dc.b    $44,$44,$44,$7c,$44,$44,$44,$00   ; H
        dc.b    $38,$10,$10,$10,$10,$10,$38,$00   ; I
        dc.b    $1c,$08,$08,$08,$08,$48,$30,$00   ; J
        dc.b    $44,$48,$50,$60,$50,$48,$44,$00   ; K
        dc.b    $40,$40,$40,$40,$40,$40,$7c,$00   ; L
        dc.b    $44,$6c,$54,$54,$44,$44,$44,$00   ; M
        dc.b    $44,$44,$64,$54,$4c,$44,$44,$00   ; N
        dc.b    $38,$44,$44,$44,$44,$44,$38,$00   ; O
        dc.b    $78,$44,$44,$78,$40,$40,$40,$00   ; P
        dc.b    $38,$44,$44,$44,$54,$48,$34,$00   ; Q
        dc.b    $78,$44,$44,$78,$50,$48,$44,$00   ; R
        dc.b    $3c,$40,$40,$38,$04,$04,$78,$00   ; S
        dc.b    $7c,$10,$10,$10,$10,$10,$10,$00   ; T
        dc.b    $44,$44,$44,$44,$44,$44,$38,$00   ; U
        dc.b    $44,$44,$44,$44,$44,$28,$10,$00   ; V
        dc.b    $44,$44,$44,$54,$54,$54,$28,$00   ; W
        dc.b    $44,$44,$28,$10,$28,$44,$44,$00   ; X
        dc.b    $44,$44,$28,$10,$10,$10,$10,$00   ; Y
        dc.b    $7c,$04,$08,$10,$20,$40,$7c,$00   ; Z
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; [
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; \
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ]
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ^
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; _

        section bss,bss

ShowScreen:     ds.l    1
DrawScreen:     ds.l    1
PendingScreen:  ds.l    1
TimeStart:      ds.l    1
RenderLines:    ds.l    1
C2PLines:       ds.l    1
FramesPerPicture: ds.w  1
TimeText:       ds.b    8
Deltas:         ds.w    CHUNKYW
Texture:        ds.b    TEXBYTES*2      ; any memory: only the processor reads it

; ---------------------------------------------------------------------------------------------------------------
; The copper list, in chip RAM.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches (OCS and ECS ignore these)
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2ca1           ; 256 pixels across, in the middle: $81 + 32
        dc.w    DIWSTOP,(LASTLINE&$ff)<<8!$a1
        dc.w    DDFSTRT,$0048           ; 16 fetches of 16 pixels, from 32 pixels in
        dc.w    DDFSTOP,$00c0
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPLCON0,$0200
        ; 16 colours along the texture's 16 steps: night, blue, sea, ice, white, gold, flame, rose.
        dc.w    COLOR00,$000,COLOR01,$124,COLOR02,$236,COLOR03,$258
        dc.w    COLOR04,$37a,COLOR05,$5ac,COLOR06,$8de,COLOR07,$eff
        dc.w    COLOR08,$fe9,COLOR09,$fc5,COLOR10,$f93,COLOR11,$e62
        dc.w    COLOR12,$c34,COLOR13,$926,COLOR14,$614,COLOR15,$302
        dc.w    (FIRSTLINE-4)<<8!$01,$fffe      ; line $28: VBlank has finished with the pointers
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0
        dc.w    BPLCON0,$4200           ; 4 bitplanes, colour on
CopperLines:
        ds.w    6*HEIGHT                ; a wait and the two modulos for each line (MakeCopperDoubling)
        dc.w    (PANELLINE-1)<<8!$07,$fffe
        dc.w    BPLCON0,$0200
        dc.w    COLOR00,$000,COLOR01,$fff
        dc.w    (PANELLINE)<<8!$07,$fffe
CopperPanel:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL1MOD,0
        dc.w    BPLCON0,$1200
        dc.w    (LASTLINE)<<8!$07,$fffe
        dc.w    BPLCON0,$0200
        dc.w    $ffff,$fffe

        section chip,bss_c

Screen1:        ds.b    SCREENBYTES
Screen2:        ds.b    SCREENBYTES
Chunky:         ds.b    C2PBYTES        ; the blitter reads these, so they're in chip RAM
Halfway:        ds.b    C2PBYTES
Panel:          ds.b    ROWBYTES*PANELHEIGHT
