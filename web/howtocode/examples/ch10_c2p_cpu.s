; ch10_c2p_cpu.s - AGA only: a rotozoomer drawn in chunky form (one byte a pixel, in fast RAM if there is some)
; every frame, and turned into 8 bitplanes by the processor: chunky to planar by merging, 32 pixels at a time.
; The panel below the picture gives the time each part took, in frames, measured with a line counter, and
; how many frames each new picture took in all. On a machine without AGA it shows nothing and ends at once.
; Left mouse button to leave.
;
; How To Code v8, chapter 10. CC0: use it for anything.
;
; Run it on an A1200 with and without fast RAM, and on something faster: the times change, and not by the same
; amount. Two settings:
;
;   SCRAMBLE  1: the rotozoomer writes each 32 pixels in a scrambled order, which saves the c2p two of its five
;             rounds of merges (C2P8Scrambled).
;   DOUBLE    1: 2x2 pixels: a chunky buffer half as wide and half as tall, made into a full-size picture by the
;             c2p (C2P8Double) and the copper, the way most A1200 effects were shown. (SCRAMBLE is then ignored.)
;
; $VER: ch10_c2p_cpu 8.0 (20.11.2026)

OS_FRIENDLY     equ     0
SCRAMBLE        equ     0
DOUBLE          equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320             ; the picture on the screen
HEIGHT          equ     176             ; 176 lines, then the panel: 200 in all, so it fits on NTSC too
ROWBYTES        equ     WIDTH/8         ; 40: a multiple of 8, as 4x fetch needs
PLANES          equ     8
ROWSTRIDE       equ     ROWBYTES*PLANES ; interleaved: each line has all eight planes' rows in turn
SCREENBYTES     equ     ROWSTRIDE*HEIGHT
        ifne    DOUBLE
CHUNKYW         equ     WIDTH/2         ; the chunky buffer: one byte for each pixel drawn
CHUNKYH         equ     HEIGHT/2
SCREENROWS      equ     HEIGHT/2        ; each row of bitplanes is shown twice
        else
CHUNKYW         equ     WIDTH
CHUNKYH         equ     HEIGHT
SCREENROWS      equ     HEIGHT
        endc
FIRSTLINE       equ     $2c
PANELLINE       equ     FIRSTLINE+HEIGHT+1      ; one blank line between the picture and the panel
PANELHEIGHT     equ     23
LASTLINE        equ     PANELLINE+PANELHEIGHT   ; $f4: the end of an NTSC display


; The texture: 256 texels across and 128 down, one byte each. Each row is stored twice over (512 bytes), and
; the whole 64K twice over (128K): then a texel's offset is u + v * 512 in a word, two offsets can be added
; without u carrying into v, and any word offset, read as a signed index from the middle, lands inside.
TEXBYTES        equ     512*128

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #263,LinesPerFrame
.pal:
        bsr     IsAGA
        bne.s   .aga
        move.l  #$fffffffe,CopperAGA    ; no AGA: the copper list ends after its first move
        st      ExitNow
        rts
.aga:
        ; Two screens on 8-byte boundaries, for 4x fetch: one shown while the other is drawn.
        move.l  #ScreenSpace+7,d0
        and.w   #$fff8,d0
        move.l  d0,ShowScreen
        add.l   #SCREENBYTES,d0
        move.l  d0,DrawScreen
        move.l  ShowScreen,PendingScreen
        st      SwapPending
        bsr     VBlank                  ; puts it in the copper list, as it will every frame

        move.l  #PanelSpace+7,d0
        and.w   #$fff8,d0
        move.l  d0,PanelStart
        lea     CopperPanel,a0
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)

        ifne    DOUBLE
        bsr     MakeCopperDoubling
        endc

        bsr     MakePalette
        bsr     MakeTexture
        bsr     WhichProcessor
        moveq   #0,d0
        moveq   #2,d1
        bsr     DrawText                ; a0 = the processor's name
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
; Is this an AGA machine? (Chapter 9.) Returns d0 = 1 (and the Z flag clear) if so, 0 if not.

IsAGA:
        move.w  DENISEID(a5),d0
        cmp.b   #$f8,d0
        bne.s   .no
        moveq   #32-1,d1
.again:
        move.w  DENISEID(a5),d2
        cmp.b   d0,d2
        bne.s   .no
        dbf     d1,.again
        moveq   #1,d0
        rts
.no:
        moveq   #0,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame (or rather, as often as it can): draw the next picture in chunky form, turn it into bitplanes in
; the screen not on show, and have VBlank show it. Each part is timed with the line counter.

Frame:
        tst.b   ExitNow                 ; no AGA: nothing to do
        bne     .done
        ; With the system running, a frame's signal can be waiting from while the last picture was being made:
        ; don't start on the screen still being shown before VBlank has swapped (see chapter 13).
.waitswap:
        tst.b   SwapPending
        beq.s   .swapped
        jsr     WaitFrame
        bra.s   .waitswap
.swapped:
        move.l  Frames,d0
        move.l  d0,d1
        sub.l   LastPicture,d1          ; frames since the last picture was shown
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

        lea     Chunky,a0
        move.l  DrawScreen,a1
        ifne    DOUBLE
        bsr     C2P8Double
        else
        ifne    SCRAMBLE
        bsr     C2P8Scrambled
        else
        bsr     C2P8
        endc
        endc
        lea     CUSTOM,a5

        bsr     LineClock
        sub.l   TimeStart,d0
        move.l  d0,C2PLines

        ; Show it at the next frame, and draw into the other screen next time.
        move.l  DrawScreen,d0
        move.l  ShowScreen,DrawScreen
        move.l  d0,ShowScreen
        move.l  d0,PendingScreen
        st      SwapPending

        bsr     ShowTimes
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame: put the screen Frame has finished into the copper list. The copper sets the
; bitplane pointers only after waiting for line $28, long after this has run, so it never sees half a change.

VBlank:
        tst.b   SwapPending
        beq.s   .none
        move.l  PendingScreen,d0
        lea     CopperBitplanes,a0
        moveq   #PLANES-1,d1
.plane:
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        add.l   #ROWBYTES,d0
        addq.l  #8,a0
        dbf     d1,.plane
        sf      SwapPending
.none:
        rts

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
; The rotozoomer. Every pixel of the picture is a texel of the texture, turned and zoomed: as x goes one pixel to
; the right, the texture position moves (du, dv); as y goes one line down, (-dv, du). Both in 8.8 fixed point.
;
; The step along a line is the same for every line, so the offsets of each pixel from the start of its line are
; worked out once, into Deltas (texel offsets: u + v * 512, so they add without u carrying into v); each line then
; has its own start, and each pixel is one table read and one texture read. (Adding the whole parts separately
; can be a texel out from the exact position, which doesn't show.)

Render:
        ; The angle turns once in 256 frames; the zoom swings between about 0.5 and 2 texels a pixel.
        move.l  Frames,d0
        lea     Sine,a0
        move.w  d0,d1
        and.w   #255,d1
        add.w   d1,d1
        move.w  0(a0,d1.w),d2           ; sin(angle), 2.14
        add.w   #64*2,d1                ; cos = sin a quarter turn on
        and.w   #511,d1
        move.w  0(a0,d1.w),d3           ; cos(angle)
        move.w  d0,d1
        mulu    #3,d1                   ; the zoom swings at 3/2 the speed the angle turns
        lsr.w   #1,d1
        and.w   #255,d1
        add.w   d1,d1
        move.w  0(a0,d1.w),d4
        muls    #192,d4
        asr.l   #8,d4
        asr.l   #6,d4                   ; sin * 192: -192 to 192
        add.w   #320,d4                 ; zoom: 128 to 512 (0.5 to 2 texels a pixel), 8.8
        ifne    DOUBLE
        add.w   d4,d4                   ; each chunky pixel is two on the screen
        endc
        muls    d4,d3
        asr.l   #8,d3
        asr.l   #6,d3                   ; du = cos * zoom, 8.8
        muls    d4,d2
        asr.l   #8,d2
        asr.l   #6,d2                   ; dv = sin * zoom
        move.w  d3,d6                   ; du
        move.w  d2,d7                   ; dv

        ; Deltas: for x from -CHUNKYW/2, the offset of (x * du, x * dv).
        lea     Deltas,a2
        move.w  #-CHUNKYW/2,d0
        muls    d6,d0                   ; u
        move.w  #-CHUNKYW/2,d1
        muls    d7,d1                   ; v
        ext.l   d6
        ext.l   d7
        move.w  #CHUNKYW-1,d5
        ifne    SCRAMBLE&(1-DOUBLE)
        moveq   #0,d4                   ; x, from the left
        lea     Scramble,a3
        endc
.delta:
        bsr     TexelOffset
        ifne    SCRAMBLE&(1-DOUBLE)
        ; Each block of 32 pixels in the order C2P8Scrambled wants: pixel x goes to where Scramble says.
        move.w  d4,d3
        and.w   #31,d3
        move.b  0(a3,d3.w),d3
        add.w   d3,d3
        move.w  d2,0(a2,d3.w)
        addq.w  #1,d4
        moveq   #31,d3
        and.w   d4,d3
        bne.s   .sameblock
        lea     32*2(a2),a2
.sameblock:
        else
        move.w  d2,(a2)+
        endc
        add.l   d6,d0
        add.l   d7,d1
        dbf     d5,.delta

        ; The centre of the picture moves across the texture.
        move.l  Frames,d0
        move.l  d0,d1
        lsl.l   #8,d0                   ; u0 = frames * 1 texel and a bit
        add.l   d1,d0
        mulu    #180,d1                 ; v0 = frames * 0.7 texels

        ; The first line's start: y = -CHUNKYH/2, so (u, v) = (u0 - y * dv, v0 + y * du).
        move.w  #CHUNKYH/2,d2
        muls    d7,d2
        add.l   d2,d0                   ; u0 + CHUNKYH/2 * dv
        move.w  #CHUNKYH/2,d2
        muls    d6,d2
        sub.l   d2,d1                   ; v0 - CHUNKYH/2 * du

        lea     Chunky,a1
        move.w  #CHUNKYH-1,d5
.line:
        bsr     TexelOffset
        lea     Texture+32768,a0        ; the offsets are signed words: from 32K in, line start + offset is
        add.l   d2,a0                   ; always inside the 128K
        lea     Deltas,a2
        moveq   #CHUNKYW/32-1,d4
.pixels:
        rept    32
        move.w  (a2)+,d3
        move.b  0(a0,d3.w),(a1)+
        endr
        dbf     d4,.pixels
        sub.l   d7,d0                   ; down a line: u -= dv, v += du
        add.l   d6,d1
        dbf     d5,.line
        rts

        ifne    SCRAMBLE&(1-DOUBLE)
; Where each of 32 pixels goes in its block, for C2P8Scrambled: pixel p to byte 4n + j, where n is made of p's
; bits 1, 0 and 2 (in that order, high to low) and j of its bits 4 and 3.
Scramble:
        dc.b    0,8,16,24,4,12,20,28,1,9,17,25,5,13,21,29,2,10,18,26,6,14,22,30,3,11,19,27,7,15,23,31
        endc

; (u, v) in d0 and d1 (8.8) to a texel offset in d2: (u & 255) + (v & 127) * 512, zero-extended to a long.
TexelOffset:
        move.l  d1,d2
        lsr.l   #8,d2
        and.l   #127,d2
        lsl.w   #8,d2
        add.w   d2,d2                   ; v * 512
        move.l  d0,d3
        lsr.l   #8,d3
        and.w   #255,d3
        or.w    d3,d2
        rts

; ---------------------------------------------------------------------------------------------------------------
; Chunky to planar, 8 bitplanes, 1x1 pixels: CHUNKYW by CHUNKYH bytes at a0 to the interleaved screen at a1.
; Changes every register but a7.
;
; Each 32 pixels (8 longwords, L0 to L7, four pixels in each) become 8 longwords, one of each bitplane. Think of
; the 256 bits as a table to be transposed. A merge swaps a group of bits between two registers: with s = 16,
; the low word of one with the high word of the other; with s = 8, every other byte; and so on to single bits.
; Five rounds of merges, s = 16, 8, 4, 2 and 1, four merges in each, transpose the whole table. Which pairs of
; registers each round merges is worked out so the work splits in two:
;
;   s = 4  on (L0,L1) (L2,L3) (L4,L5) (L6,L7)
;   then the even ones, L0 L2 L4 L6, on their own: s = 16 and s = 2 on (L0,L4) (L2,L6); s = 8 and s = 1 on
;   (L0,L2) (L4,L6) - and the odd ones the same.
;
; That leaves bitplane 7 in L0, 3 in L1, 6 in L2, 2 in L3, 5 in L4, 1 in L5, 4 in L6, 0 in L7.
;
; The writes to chip RAM are spread through the work, not bunched at the end: a 68020 or 68030 can go on with
; the next instructions (from its cache) while a write it has started waits for its turn at chip RAM. So the
; odd half's four planes are written during the next block's first round, and the even half's during the odd
; half's merges. a6 is where the last block went; a2-a5 hold the planes waiting to be written.

; A merge: swap the bits of \1 picked by the mask \4 with the bits of \2 that are \3 places to their left.
MERGE           macro
        move.l  \2,d5
        lsr.l   #\3,d5
        eor.l   \1,d5
        and.l   #\4,d5
        eor.l   d5,\1
        lsl.l   #\3,d5
        eor.l   d5,\2
        endm

; The same with s = 16: \1's low word swapped with \2's high word, without a spare register.
MERGE16         macro
        swap    \2
        eor.w   \1,\2
        eor.w   \2,\1
        eor.w   \1,\2
        swap    \2
        endm

; ---- c2p8 begin
C2P8:
        lea     C2PScratch,a6           ; the "last block" for the first: its four writes land here
        move.w  #CHUNKYH-1,d7
.row:
        moveq   #CHUNKYW/32-1,d6
.block:
        ; s = 4 on each pair, writing the last block's odd planes, and keeping this block's odd longwords in
        ; a2-a5 in their place.
        move.l  (a0)+,d0
        move.l  (a0)+,d4
        MERGE   d0,d4,4,$0f0f0f0f
        move.l  a2,3*ROWBYTES(a6)       ; L1: bitplane 3
        move.l  d4,a2
        move.l  (a0)+,d1
        move.l  (a0)+,d4
        MERGE   d1,d4,4,$0f0f0f0f
        move.l  a3,2*ROWBYTES(a6)       ; L3: bitplane 2
        move.l  d4,a3
        move.l  (a0)+,d2
        move.l  (a0)+,d4
        MERGE   d2,d4,4,$0f0f0f0f
        move.l  a4,1*ROWBYTES(a6)       ; L5: bitplane 1
        move.l  d4,a4
        move.l  (a0)+,d3
        move.l  (a0)+,d4
        MERGE   d3,d4,4,$0f0f0f0f
        move.l  a5,0*ROWBYTES(a6)       ; L7: bitplane 0
        move.l  d4,a5

        ; The even longwords: d0-d3 = L0 L2 L4 L6.
        MERGE16 d0,d2
        MERGE16 d1,d3
        MERGE   d0,d2,2,$33333333
        MERGE   d1,d3,2,$33333333
        MERGE   d0,d1,8,$00ff00ff
        MERGE   d2,d3,8,$00ff00ff
        MERGE   d0,d1,1,$55555555
        MERGE   d2,d3,1,$55555555

        ; Swap the finished even planes for the odd longwords, and do those, writing the even planes meanwhile.
        exg     d0,a2
        exg     d1,a3
        exg     d2,a4
        exg     d3,a5
        MERGE16 d0,d2
        MERGE16 d1,d3
        move.l  a2,7*ROWBYTES(a1)       ; L0: bitplane 7
        MERGE   d0,d2,2,$33333333
        MERGE   d1,d3,2,$33333333
        move.l  a3,6*ROWBYTES(a1)       ; L2: bitplane 6
        MERGE   d0,d1,8,$00ff00ff
        MERGE   d2,d3,8,$00ff00ff
        move.l  a4,5*ROWBYTES(a1)       ; L4: bitplane 5
        MERGE   d0,d1,1,$55555555
        MERGE   d2,d3,1,$55555555
        move.l  a5,4*ROWBYTES(a1)       ; L6: bitplane 4

        ; The odd planes wait for the next block.
        move.l  d0,a2
        move.l  d1,a3
        move.l  d2,a4
        move.l  d3,a5
        move.l  a1,a6
        addq.l  #4,a1
        dbf     d6,.block
        add.w   #ROWSTRIDE-ROWBYTES,a1  ; on to the next line's first plane
        dbf     d7,.row

        ; The last block's odd planes.
        move.l  a2,3*ROWBYTES(a6)
        move.l  a3,2*ROWBYTES(a6)
        move.l  a4,1*ROWBYTES(a6)
        move.l  a5,0*ROWBYTES(a6)
        rts
; ---- c2p8 end

; ---------------------------------------------------------------------------------------------------------------
; Chunky to planar, 8 bitplanes, 1x1 pixels, from scrambled chunky: as C2P8, but without the rounds with s = 16 and
; s = 8. Those two only move whole pixels about (two at a time, and one), so if each 32 pixels are written to the
; chunky buffer already in the order those rounds would leave them, they needn't be done at all. That order is
; Scramble's: the bytes of the 8 longwords hold pixels 0 8 16 24, 4 12 20 28, 1 9 17 25, 5 13 21 29, 2 10 18 26,
; 6 14 22 30, 3 11 19 27, 7 15 23 31. The rotozoomer gets it for nothing, by putting its table of offsets in
; that order; anything drawing in the chunky buffer has to know it.

; ---- c2p8scrambled begin
C2P8Scrambled:
        lea     C2PScratch,a6           ; the "last block" for the first: its four writes land here
        move.w  #CHUNKYH-1,d7
.row:
        moveq   #CHUNKYW/32-1,d6
.block:
        ; s = 4 on each pair, writing the last block's odd planes, and keeping this block's odd longwords in
        ; a2-a5 in their place.
        move.l  (a0)+,d0
        move.l  (a0)+,d4
        MERGE   d0,d4,4,$0f0f0f0f
        move.l  a2,3*ROWBYTES(a6)       ; L1: bitplane 3
        move.l  d4,a2
        move.l  (a0)+,d1
        move.l  (a0)+,d4
        MERGE   d1,d4,4,$0f0f0f0f
        move.l  a3,2*ROWBYTES(a6)       ; L3: bitplane 2
        move.l  d4,a3
        move.l  (a0)+,d2
        move.l  (a0)+,d4
        MERGE   d2,d4,4,$0f0f0f0f
        move.l  a4,1*ROWBYTES(a6)       ; L5: bitplane 1
        move.l  d4,a4
        move.l  (a0)+,d3
        move.l  (a0)+,d4
        MERGE   d3,d4,4,$0f0f0f0f
        move.l  a5,0*ROWBYTES(a6)       ; L7: bitplane 0
        move.l  d4,a5

        ; The even longwords: d0-d3 = L0 L2 L4 L6. (No s = 16 or 8: the pixels were written already moved.)
        MERGE   d0,d2,2,$33333333
        MERGE   d1,d3,2,$33333333
        MERGE   d0,d1,1,$55555555
        MERGE   d2,d3,1,$55555555

        ; Swap the finished even planes for the odd longwords, and do those, writing the even planes meanwhile.
        exg     d0,a2
        exg     d1,a3
        exg     d2,a4
        exg     d3,a5
        move.l  a2,7*ROWBYTES(a1)       ; L0: bitplane 7
        MERGE   d0,d2,2,$33333333
        move.l  a3,6*ROWBYTES(a1)       ; L2: bitplane 6
        MERGE   d1,d3,2,$33333333
        move.l  a4,5*ROWBYTES(a1)       ; L4: bitplane 5
        MERGE   d0,d1,1,$55555555
        move.l  a5,4*ROWBYTES(a1)       ; L6: bitplane 4
        MERGE   d2,d3,1,$55555555

        ; The odd planes wait for the next block.
        move.l  d0,a2
        move.l  d1,a3
        move.l  d2,a4
        move.l  d3,a5
        move.l  a1,a6
        addq.l  #4,a1
        dbf     d6,.block
        add.w   #ROWSTRIDE-ROWBYTES,a1  ; on to the next line's first plane
        dbf     d7,.row

        ; The last block's odd planes.
        move.l  a2,3*ROWBYTES(a6)
        move.l  a3,2*ROWBYTES(a6)
        move.l  a4,1*ROWBYTES(a6)
        move.l  a5,0*ROWBYTES(a6)
        rts
; ---- c2p8scrambled end

; ---------------------------------------------------------------------------------------------------------------
; Chunky to planar for 2x1 pixels, 8 bitplanes: each chunky pixel is two pixels across. CHUNKYW by CHUNKYH bytes
; at a0 to the interleaved screen at a1, a row of bitplanes for each chunky line (the copper shows each twice).
; Changes every register but a7.
;
; 16 chunky pixels (4 longwords, L0 to L3) make 32 pixels of each bitplane. Four rounds of merges (s = 16 and 4
; on (L0,L2) (L1,L3); s = 8 and 2 on (L0,L1) (L2,L3)) leave two bitplanes in each register, in alternate bits:
; bitplanes 6 and 7 in L0 (7 in the odd bits), 4 and 5 in L1, 2 and 3 in L2, 0 and 1 in L3. Each half, its bits
; doubled, is a bitplane's 32 pixels: the doubling is the merge that would have been the fifth round.

; Doubles \1's even bits into a bitplane (left in \1) and its odd bits into another (in \2). d5 is changed.
DOUBLEBITS      macro
        move.l  \1,\2
        and.l   #$55555555,\1           ; the even bits
        eor.l   \1,\2                   ; the odd bits
        move.l  \1,d5
        add.l   d5,d5
        or.l    d5,\1                   ; even bits doubled
        move.l  \2,d5
        lsr.l   #1,d5
        or.l    d5,\2                   ; odd bits doubled
        endm

; ---- c2p8double begin
C2P8Double:
        move.w  #CHUNKYH-1,d7
.row:
        moveq   #CHUNKYW/16-1,d6
.block:
        movem.l (a0)+,d0-d3
        MERGE16 d0,d2
        MERGE16 d1,d3
        MERGE   d0,d2,4,$0f0f0f0f
        MERGE   d1,d3,4,$0f0f0f0f
        MERGE   d0,d1,8,$00ff00ff
        MERGE   d2,d3,8,$00ff00ff
        MERGE   d0,d1,2,$33333333
        MERGE   d2,d3,2,$33333333
        DOUBLEBITS d0,d4
        move.l  d0,6*ROWBYTES(a1)
        move.l  d4,7*ROWBYTES(a1)
        DOUBLEBITS d1,d4
        move.l  d1,4*ROWBYTES(a1)
        move.l  d4,5*ROWBYTES(a1)
        DOUBLEBITS d2,d4
        move.l  d2,2*ROWBYTES(a1)
        move.l  d4,3*ROWBYTES(a1)
        DOUBLEBITS d3,d4
        move.l  d3,0*ROWBYTES(a1)
        move.l  d4,1*ROWBYTES(a1)
        addq.l  #4,a1
        dbf     d6,.block
        add.w   #ROWSTRIDE-ROWBYTES,a1
        dbf     d7,.row
        rts
; ---- c2p8double end

; ---------------------------------------------------------------------------------------------------------------
; With DOUBLE: each row of bitplanes shown twice, by the copper changing the modulos on every line. After the
; first line of a pair, the modulo takes the pointers back to the start of the row (-ROWBYTES); after the second,
; on past the other seven planes' rows to the next (ROWSTRIDE - ROWBYTES).

        ifne    DOUBLE
MakeCopperDoubling:
        lea     CopperLines,a0
        move.w  #FIRSTLINE,d0
        move.w  #HEIGHT-1,d1
.line:
        move.w  d0,d2
        lsl.w   #8,d2
        or.w    #$07,d2                 ; wait for the start of the line, before its fetch begins
        move.w  d2,(a0)+
        move.w  #$fffe,(a0)+
        move.w  #ROWSTRIDE-ROWBYTES,d2
        btst    #0,d1                   ; counting down from an odd number: odd on the first of a pair
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
        endc

; ---------------------------------------------------------------------------------------------------------------
; The texture: u XOR (v * 2), 256 by 128 texels, every row twice over and the whole twice over (see TEXBYTES).

MakeTexture:
        lea     Texture,a0
        move.w  #256-1,d7               ; 256 rows: the 128 twice
.row:
        move.w  d7,d1
        not.w   d1
        add.w   d1,d1                   ; v * 2 (the top bit falls off as a byte)
        move.w  #512-1,d6
        moveq   #0,d0                   ; u
.texel:
        move.b  d0,d2
        eor.b   d1,d2
        move.b  d2,(a0)+
        addq.b  #1,d0
        dbf     d6,.texel
        dbf     d7,.row
        rts

; ---------------------------------------------------------------------------------------------------------------
; The palette, into the copper list: 256 colours of 24 bits, smooth steps between eight key colours (as in
; chapter 9's ch09_256colours.s), so the texture's 256 values shade smoothly.

MakePalette:
        lea     KeyColours,a0
        lea     Palette,a1
        moveq   #8-1,d7
.key:
        move.l  (a0)+,d0
        move.l  (a0),d1
        tst.w   d7
        bne.s   .notlast
        move.l  KeyColours,d1
.notlast:
        moveq   #0,d2
.step:
        moveq   #0,d6
        moveq   #16,d5
.component:
        move.l  d0,d3
        lsr.l   d5,d3
        and.w   #$ff,d3
        move.l  d1,d4
        lsr.l   d5,d4
        and.w   #$ff,d4
        sub.w   d3,d4
        muls    d2,d4
        asr.l   #5,d4
        add.w   d3,d4
        and.l   #$ff,d4
        lsl.l   d5,d4
        or.l    d4,d6
        subq.w  #8,d5
        bpl.s   .component
        move.l  d6,(a1)+
        addq.w  #1,d2
        cmp.w   #32,d2
        blo.s   .step
        dbf     d7,.key

        ; Into the copper list: for each bank of 32, the high halves, then (LOCT set) the low halves.
        lea     CopperPalette,a0
        lea     Palette,a1
        moveq   #0,d0
.bank:
        move.w  d0,d1
        ror.w   #3,d1
        or.w    #$0c20,d1
        move.w  #BPLCON3,(a0)+
        move.w  d1,(a0)+
        moveq   #4,d4                   ; the high nibbles: shift 4
        bsr.s   .registers
        or.w    #$0200,d1
        move.w  #BPLCON3,(a0)+
        move.w  d1,(a0)+
        moveq   #0,d4                   ; the low nibbles
        bsr.s   .registers
        lea     32*4(a1),a1
        addq.w  #1,d0
        cmp.w   #8,d0
        blo.s   .bank
        rts
.registers:
        move.w  #COLOR00,d2
        moveq   #32-1,d3
        move.l  a1,a2
.register:
        move.l  (a2)+,d5                ; $00RrGgBb
        lsr.l   d4,d5
        moveq   #0,d6
        move.b  d5,d6
        and.w   #$0f,d6                 ; blue
        lsr.l   #4,d5
        move.w  d5,d7
        and.w   #$f0,d7                 ; green
        or.w    d7,d6
        lsr.l   #4,d5
        and.w   #$f00,d5                ; red
        or.w    d5,d6
        move.w  d2,(a0)+
        move.w  d6,(a0)+
        addq.w  #2,d2
        dbf     d3,.register
        rts

; ---------------------------------------------------------------------------------------------------------------
; The panel: the times, in frames to two places, from the line counts.

ShowTimes:
        move.l  RenderLines,d0
        moveq   #7,d1
        bsr.s   ShowFrames
        move.l  C2PLines,d0
        moveq   #21,d1
        bsr.s   ShowFrames
        moveq   #0,d0
        move.w  FramesPerPicture,d0
        cmp.w   #99,d0
        bls.s   .fits
        moveq   #99,d0
.fits:
        divu    #10,d0                  ; tens, and the units in the top word
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
        moveq   #37,d0
        moveq   #12,d1
        bra     DrawText

; A time in lines (d0) as frames to two places, "nn.nn", at column d1 of the times' row.
ShowFrames:
        move.w  d1,-(sp)
        move.w  LinesPerFrame,d1
        mulu    #100,d0                 ; hundredths of a line (d0 is under 65536 lines)
        moveq   #0,d2
        move.w  d1,d2
        lsr.w   #1,d2
        add.l   d2,d0                   ; rounded to the nearest hundredth of a frame
        divu    d1,d0
        and.l   #$ffff,d0
        cmp.l   #9999,d0
        bls.s   .digits
        move.l  #9999,d0
.digits:
        lea     TimeText+5,a0
        clr.b   (a0)
        moveq   #5-1,d2                 ; five places, one of them the point
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
        cmp.b   #"0",(a0)               ; no nought in front
        bne.s   .draw
        move.b  #" ",(a0)
.draw:
        move.w  (sp)+,d0
        moveq   #12,d1
        bra     DrawText

; The processor, by AttnFlags (chapter 14): a0 = its name.
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
        lea     Name68000,a0
.known:
        rts

; Text, 8 by 8 pixels a character, into the panel. a0 = the string (ending in zero), d0 = the column (0 to 39),
; d1 = the row, in pixels.
DrawText:
        move.l  PanelStart,a1
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

; The key colours, 24 bits each: night blue, sea, ice, white, gold, flame, rose, violet.
KeyColours:
        dc.l    $000c1a40,$00127a9a,$0070d0e0,$00f4f4f0
        dc.l    $00f0c040,$00e05010,$00a01050,$00381060

; A whole turn of sine in 256 words: round(sin(2*pi*i/256) * 16384), 2.14 fixed point (16384 as 16383).
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
Name68020:      dc.b    "68020",0
Name68030:      dc.b    "68030",0
Name68040:      dc.b    "68040",0
Name68060:      dc.b    "68060",0
        ifne    DOUBLE
ModeText:       dc.b    "CPU C2P, 8 PLANES, 2X2 PIXELS",0
        else
        ifne    SCRAMBLE
ModeText:       dc.b    "CPU C2P, 8 PLANES, 1X1 SCRAMBLED",0
        else
ModeText:       dc.b    "CPU C2P, 8 PLANES, 1X1 PIXELS",0
        endc
        endc
LabelsText:     dc.b    "RENDER       FR  C2P       FR  EVERY",0
        even

; An 8 by 8 font for the characters from the space to the underline (ASCII 32 to 95), made for How To Code
; v8 (it's chapter 14's): one byte a row, the top row first.
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
PanelStart:     ds.l    1
TimeStart:      ds.l    1
RenderLines:    ds.l    1
C2PLines:       ds.l    1
FramesPerPicture: ds.w  1
TimeText:       ds.b    8
Palette:        ds.l    256
Deltas:         ds.w    CHUNKYW
C2PScratch:     ds.l    ROWSTRIDE/4     ; where the first block's "last block" writes go
; These two are in any memory: fast RAM, if the machine has some.
Chunky:         ds.b    CHUNKYW*CHUNKYH
Texture:        ds.b    TEXBYTES*2

; ---------------------------------------------------------------------------------------------------------------
; The copper list, in chip RAM.

        section copper,data_c

Copper:
        dc.w    BPLCON0,$0201           ; no bitplanes until the rest is ready (without AGA, this is all);
                                        ; ECSENA on, so BRDRBLNK keeps the border black from the top
CopperAGA:
        dc.w    FMODE,$0003             ; bitplanes fetched 64 bits at a time (4x)
        dc.w    BPLCON3,$0c20           ; bank 0, LOCT clear, BRDRBLNK: the border black
        dc.w    BPLCON4,$0011
        dc.w    DIWSTRT,$2c81
        dc.w    DIWSTOP,(LASTLINE&$ff)<<8!$c1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00b8           ; 4x: five fetches of 64 pixels
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,ROWSTRIDE-ROWBYTES
        dc.w    BPL2MOD,ROWSTRIDE-ROWBYTES
CopperPalette:
        ds.w    (2+32*2)*2*8            ; filled in by MakePalette: 8 banks of 32 colours, high and low halves
        dc.w    BPLCON3,$0c20
        dc.w    (FIRSTLINE-4)<<8!$01,$fffe      ; line $28: VBlank has finished with the pointers
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0
        dc.w    BPL5PTH,0,BPL5PTL,0
        dc.w    BPL6PTH,0,BPL6PTL,0
        dc.w    BPL7PTH,0,BPL7PTH+2,0
        dc.w    BPL8PTH,0,BPL8PTH+2,0
        dc.w    BPLCON0,$0211           ; 8 bitplanes, colour on, ECSENA (for BRDRBLNK)
        ifne    DOUBLE
CopperLines:
        ds.w    6*HEIGHT                ; a wait and the two modulos for each line (MakeCopperDoubling)
        endc
        ; The panel: one bitplane, after a blank line, with colours 0 and 1 black and white for it.
        dc.w    (PANELLINE-1)<<8!$07,$fffe
        dc.w    BPLCON0,$0201
        dc.w    COLOR00,$0000,COLOR01,$0fff     ; the high halves (and so, for now, the low ones too)
        dc.w    (PANELLINE)<<8!$07,$fffe
CopperPanel:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL1MOD,0
        dc.w    BPLCON0,$1201
        dc.w    (LASTLINE)<<8!$07,$fffe
        dc.w    BPLCON0,$0201
        dc.w    $ffff,$fffe

        section screen,bss_c

ScreenSpace:    ds.b    SCREENBYTES*2+8
PanelSpace:     ds.b    ROWBYTES*PANELHEIGHT+8
