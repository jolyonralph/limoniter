; ch05_cia_timer.s - a CIA-B timer interrupt at 50 Hz, worked out from the machine's E clock so it's 50 Hz on PAL
; and NTSC alike. Two bars grow: the top one a pixel a frame, the bottom one a pixel a timer tick, with a mark
; every 50 pixels. On PAL they keep together; on NTSC the frame bar runs ahead, as there are 60 frames a second.
; Each tick also lightens the background from the moment it comes, so you can see where in the frame it lands:
; the timer isn't tied to the display, and that line drifts. Left mouse button to leave.
;
; How To Code v8, chapter 5. CC0: use it for anything.
;
; Build and run it with F9. Set OS_FRIENDLY to 1 to keep the system running underneath: then the timer is
; allocated from ciab.resource (AddICRVector), timer A if it's free and timer B if not, and the resource's own
; level 6 handler calls ours.
;
; $VER: ch05_cia_timer 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
ROWBYTES        equ     WIDTH/8
TICK_HZ         equ     50              ; the timer's rate, in ticks a second
BARLINES        equ     16
FRAMEBAR        equ     60              ; where the bars are, in lines from the top of the screen
TICKBAR         equ     110

PAL_ECLOCK      equ     709379          ; the CIAs' clock (the E clock): a tenth of the 68000's, in Hz
NTSC_ECLOCK     equ     715909

; The CIA's registers' bits as masks, by the NDK's names (hardware/cia.i; hardware.i has the bit numbers).
CIAICRF_TA      equ     $01
CIAICRF_SETCLR  equ     $80
CIACRAF_START   equ     $01
CIACRAF_PBON    equ     $02
CIACRAF_OUTMODE equ     $04
CIACRAF_LOAD    equ     $10
CIACRAF_SPMODE  equ     $40
CIACRAF_TODIN   equ     $80
CIACRBF_ALARM   equ     $80

; Stopping a timer: these bits of its control register are kept, and the rest cleared - so it stops, runs
; continuously (not one-shot) and counts the E clock. The other bits belong to other things: SPMODE, for one,
; sets the direction of CIA-A's serial port, which is the keyboard's. (The same masks as the RKM's example.)
STOPA_AND       equ     CIACRAF_TODIN!CIACRAF_PBON!CIACRAF_OUTMODE!CIACRAF_SPMODE
STOPB_AND       equ     CIACRBF_ALARM!CIACRAF_PBON!CIACRAF_OUTMODE
START_OR        equ     CIACRAF_START!CIACRAF_LOAD      ; start, loading the count from the latch first

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        ; All eight sprites at an empty sprite. Sprite DMA may be on (with the system running it usually is,
        ; for the mouse pointer), and a sprite pointer nobody has set shows whatever it happens to point at.
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
        move.w  #$f4c1,CopperWindowEnd+2
.pal:
        ; A mark under each bar every 50 pixels: a second, for the timer's bar.
        lea     Screen+(FRAMEBAR+BARLINES+1)*ROWBYTES,a0
        bsr     DrawMarks
        lea     Screen+(TICKBAR+BARLINES+1)*ROWBYTES,a0
        bsr     DrawMarks

        ; How many E clock ticks make one timer tick, rounded: on PAL 709379/50 = 14187.58, so 14188.
        ; Kickstart 2 and later know the E clock's rate exactly; Kickstart 1.3 doesn't, so there the PAL or
        ; NTSC setting decides.
        move.l  ExecBase.w,a6
        move.l  #PAL_ECLOCK,d0
        cmp.w   #36,LIB_VERSION(a6)
        blo.s   .oldkick
        move.l  ex_EClockFrequency(a6),d0
        bra.s   .eclock
.oldkick:
        tst.b   PalDisplay
        bne.s   .eclock
        move.l  #NTSC_ECLOCK,d0
.eclock:
        add.l   #TICK_HZ/2,d0
        divu    #TICK_HZ,d0
        move.w  d0,TimerCount

        ifeq    OS_FRIENDLY
; ---- Taking the machine: CIA-B's timer A, and our own level 6 interrupt -----------------------------------------

        ; The level 6 vector (VectorBase+$78) is ours now; the old one goes back in TimerExit.
        move.l  VectorBase,a0
        move.l  $78(a0),OldLevel6
        move.l  #TimerLevel6,$78(a0)

        move.l  #CIAB+CIACRA,TimerControl
        move.l  #CIAB+CIATALO,TimerLow
        move.b  #STOPA_AND,TimerStop
        lea     CIAB,a1
        move.b  #CIAICRF_TA,CIAICR(a1)  ; timer A's interrupt off in the CIA (bit 7 clear: clear the bits given)
        bsr     StartTimer
        tst.b   CIAICR(a1)              ; reading the ICR clears any request left from before
        move.b  #CIAICRF_SETCLR!CIAICRF_TA,CIAICR(a1)   ; and its interrupt on (bit 7 set: set them)

        ; Level 6 on in Paula. The master switch (INTEN) is still off: the startup turns it on once Init returns.
        move.w  #INTF_EXTER,INTREQ(a5)
        move.w  #INTF_SETCLR!INTF_EXTER,INTENA(a5)

        else
; ---- Keeping the system running: a timer from ciab.resource -----------------------------------------------------

        move.l  ExecBase.w,a6
        lea     CiabName,a1
        jsr     _LVOOpenResource(a6)    ; (there's no CloseResource)
        move.l  d0,CiabBase
        beq     .notimer

        ; Interrupts off while the timer is claimed and set going, so its first interrupt finds it ready.
        jsr     _LVODisable(a6)
        move.l  CiabBase,a6
        moveq   #CIAICRB_TA,d0          ; timer A, if no one has it
        lea     TimerInterrupt,a1
        jsr     _LVOAddICRVector(a6)
        tst.l   d0
        bne.s   .tryb
        move.w  #CIAICRB_TA,TimerBit
        move.l  #CIAB+CIACRA,TimerControl
        move.l  #CIAB+CIATALO,TimerLow
        move.b  #STOPA_AND,TimerStop
        bra.s   .got
.tryb:
        moveq   #CIAICRB_TB,d0          ; else timer B
        lea     TimerInterrupt,a1
        jsr     _LVOAddICRVector(a6)
        tst.l   d0
        bne.s   .taken
        move.w  #CIAICRB_TB,TimerBit
        move.l  #CIAB+CIACRB,TimerControl
        move.l  #CIAB+CIATBLO,TimerLow
        move.b  #STOPB_AND,TimerStop
.got:
        bsr     StartTimer
        ; Any request left from whoever had the timer before is cleared, through the resource (never by
        ; reading the ICR: that would lose the requests of everyone else using the CIA).
        moveq   #0,d0
        move.w  TimerBit,d1
        bset    d1,d0                   ; bit 7 clear: clear the request
        jsr     _LVOSetICR(a6)
        move.l  ExecBase.w,a6
        jsr     _LVOEnable(a6)
        bra.s   .started
.taken:
        ; Both of CIA-B's timers are someone else's: give up, cleanly.
        move.l  ExecBase.w,a6
        jsr     _LVOEnable(a6)
.notimer:
        st      ExitNow
        rts
.started:
        endc

        move.l  #TimerExit,ExitHook     ; the startup calls it as the demo ends, to take the timer away
        rts

; ---------------------------------------------------------------------------------------------------------------
; Stops the timer, sets its count and starts it again. The control register is changed by single instructions
; (and.b, or.b), which no interrupt can come in the middle of.

StartTimer:
        move.l  TimerControl,a0
        move.b  TimerStop,d0
        and.b   d0,(a0)                 ; stopped, continuous, counting the E clock
        move.l  TimerLow,a0
        move.w  TimerCount,d0
        move.b  d0,(a0)                 ; the latch's low byte,
        lsr.w   #8,d0
        move.b  d0,$100(a0)             ; then its high byte (the next register)
        move.l  TimerControl,a0
        or.b    #START_OR,(a0)          ; the count loaded from the latch, and going
        rts

; ---------------------------------------------------------------------------------------------------------------
; Called by the startup as the demo ends (a5 = CUSTOM), while our interrupt is still in place.

TimerExit:
        move.l  TimerControl,a0
        move.b  TimerStop,d0
        and.b   d0,(a0)                 ; the timer stopped
        ifeq    OS_FRIENDLY
        move.w  #INTF_EXTER,INTENA(a5)  ; level 6 off (the startup puts INTENA back as it was, later)
        move.b  #CIAICRF_TA,CIAB+CIAICR ; timer A's interrupt off in the CIA, as it was
        move.l  VectorBase,a0
        move.l  OldLevel6,$78(a0)       ; the system's level 6 handler back
        else
        move.l  CiabBase,a6
        move.w  TimerBit,d0
        lea     TimerInterrupt,a1
        jsr     _LVORemICRVector(a6)    ; the timer given back (and its interrupt off)
        endc
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the two bars, a pixel for each frame and for each tick, starting again every 256.

Frame:
        move.l  Frames,d0               ; counted by the frame interrupt
        lea     Screen+FRAMEBAR*ROWBYTES,a0
        bsr     DrawBar
        move.l  Ticks,d0                ; counted by the timer interrupt: one instruction reads all of it, so
        lea     Screen+TICKBAR*ROWBYTES,a0      ; it can't change half way through
        bsr     DrawBar
        rts

; Draws a bar d0 mod 256 pixels long, BARLINES lines tall, at a0.
DrawBar:
        and.w   #255,d0
        move.w  d0,d1
        lsr.w   #3,d1                   ; whole bytes
        and.w   #7,d0
        move.w  #$ff00,d2
        lsr.w   d0,d2                   ; the last byte: as many pixels on as are left over
        lea     BarRow,a1
        moveq   #0,d0
.byte:
        cmp.w   d1,d0
        blo.s   .full
        beq.s   .part
        clr.b   (a1)+
        bra.s   .next
.full:
        st      (a1)+
        bra.s   .next
.part:
        move.b  d2,(a1)+
.next:
        addq.w  #1,d0
        cmp.w   #ROWBYTES,d0
        blo.s   .byte
        ; Then the row onto each of the bar's lines.
        moveq   #BARLINES-1,d1
.line:
        lea     BarRow,a1
        moveq   #ROWBYTES/4-1,d0
.long:
        move.l  (a1)+,(a0)+
        dbf     d0,.long
        dbf     d1,.line
        rts

; Draws marks four lines tall every 50 pixels, at a0.
DrawMarks:
        moveq   #4-1,d1
.line:
        moveq   #0,d0
.mark:
        move.w  d0,d2
        lsr.w   #3,d2
        moveq   #7,d3
        and.w   d0,d3
        eor.w   #7,d3                   ; bit 7 is the leftmost pixel of a byte
        bset    d3,(a0,d2.w)
        add.w   #50,d0
        cmp.w   #256,d0
        blo.s   .mark
        lea     ROWBYTES(a0),a0
        dbf     d1,.line
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame. Nothing to do here: the copper puts the background back.

VBlank:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The tick's work, the same both ways: count it, and lighten the background from here to the end of the frame.
; Changes nothing but memory and the colour.

TimerWork:
        addq.l  #1,Ticks
        move.w  #$035,CUSTOM+COLOR00
        rts

        ifeq    OS_FRIENDLY
; ---------------------------------------------------------------------------------------------------------------
; Taking the machine: the level 6 interrupt. CIA-B (and the expansion bus) raise it.

TimerLevel6:
        movem.l d0/a0,-(sp)
        lea     CUSTOM,a0
        ; Read CIA-B's ICR first: that clears the CIA's requests and lets its interrupt line go. While the line
        ; is held, clearing EXTER doesn't last: it's set again at once, and the interrupt comes twice. Reading
        ; the ICR clears everything it was showing, so look at every bit you care about in this one read.
        move.b  CIAB+CIAICR,d0
        move.w  #INTF_EXTER,INTREQ(a0)  ; then acknowledged, twice, for fast processors
        move.w  #INTF_EXTER,INTREQ(a0)
        btst    #CIAICRB_TA,d0
        beq.s   .nottimer
        bsr     TimerWork
.nottimer:
        movem.l (sp)+,d0/a0
        rte

        else
; ---------------------------------------------------------------------------------------------------------------
; Keeping the system running: called by ciab.resource's level 6 handler, which has read the ICR and found our
; bit set. a1 = is_Data. It may change d0, d1, a0 and a1, and must keep the rest; it simply returns.

TimerCode:
        bsr     TimerWork
        rts

        endc

        section data,data

Ticks:          dc.l    0               ; written by the timer interrupt, read by Frame
OldLevel6:      dc.l    0
CiabBase:       dc.l    0
TimerControl:   dc.l    0               ; the timer's control register, CRA or CRB
TimerLow:       dc.l    0               ; its latch's low byte (the high byte is the register after)
TimerCount:     dc.w    0               ; E clock ticks to a timer tick
TimerBit:       dc.w    0               ; CIAICRB_TA or CIAICRB_TB
TimerStop:      dc.b    0               ; STOPA_AND or STOPB_AND
        even

        ifne    OS_FRIENDLY
TimerInterrupt:
        dc.l    0,0                     ; the node's links
        dc.b    NT_INTERRUPT,0
        dc.l    TimerName               ; a name, so anyone can see who has the timer
        dc.l    0,TimerCode             ; is_Data, is_Code
TimerName:      dc.b    "How To Code v8 CIA timer",0
CiabName:       dc.b    "ciab.resource",0
        even
        endc

        section bss,bss

BarRow:         ds.b    ROWBYTES

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the screen, in chip RAM, where the chips can read them.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have (they ignore these)
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200           ; one bitplane, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    COLOR00,$0012           ; the background, dark again for the new frame, until the next tick
        dc.w    COLOR01,$0f80           ; the frame bar
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        dc.w    ($2c+TICKBAR-8)<<8!$07,$fffe
        dc.w    COLOR01,$00cf           ; the tick bar
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

        section screen,bss_c

Screen:         ds.b    ROWBYTES*HEIGHT
