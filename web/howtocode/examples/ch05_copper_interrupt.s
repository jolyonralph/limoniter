; ch05_copper_interrupt.s - the copper raises an interrupt at a line of its choosing, twice a frame, and the
; processor changes the background colour there. The copper marks the moment it raises it with a short white
; dash, so you can see how much later the processor's change comes: that gap is the interrupt's latency, about
; one lores pixel for every 68000 clock cycle. The ruler's lines are 16 pixels apart. The band moves up and down.
; Left mouse button to leave.
;
; How To Code v8, chapter 5. CC0: use it for anything.
;
; Build and run it with F9. Set OS_FRIENDLY to 1 to keep the system running underneath: then the interrupt is a
; server on exec's copper interrupt chain (AddIntServer), and the gap grows, as exec decides whose it is first.
;
; $VER: ch05_copper_interrupt 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
ROWBYTES        equ     WIDTH/8
BANDLINES       equ     24              ; how tall the band is
BANDTOP         equ     $50             ; how high and low it bounces, in display lines ($2c is the top line)
BANDBOTTOM      equ     $d8             ; (kept below line 255, so a plain wait reaches it)
INTHPOS         equ     $51             ; where on the line the copper raises the interrupt (colour clocks):
                                        ; near the left, so the gap after it can be seen on the same line

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; The ruler: one row of bitplane, a line every 16 pixels, shown on every line (the modulo of -ROWBYTES
        ; takes the bitplane pointer back to the row's start at the end of each line).
        move.l  #Ruler,d0
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

        ; NTSC has 200 lines: the display window ends at line $f4 instead of $2c (that is, $12c).
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
.pal:
        move.l  #CopperExit,ExitHook    ; the startup calls it as the demo ends, to take the interrupt away

        ifeq    OS_FRIENDLY
        ; Taking the machine: the level 3 vector already holds the startup's own handler, which counts the
        ; frames. Ours goes in front of it, and passes every level 3 interrupt on to it after looking for its
        ; own. The startup puts the system's vector back as it ends; CopperExit puts the startup's back first.
        move.l  VectorBase,a0
        move.l  $6c(a0),NextLevel3
        move.l  #CopperLevel3,$6c(a0)
        ; The copper interrupt on. The master switch (INTEN) is still off: the startup turns it on with the
        ; frame interrupt once Init returns.
        move.w  #INTF_SETCLR!INTF_COPER,INTENA(a5)
        else
        ; Keeping the system running: a server on exec's copper interrupt chain. AddIntServer turns the
        ; copper interrupt on in INTENA, as ours is the first server on the chain.
        move.l  ExecBase.w,a6
        moveq   #INTB_COPER,d0
        lea     CopperServer,a1
        jsr     _LVOAddIntServer(a6)
        endc
        rts

; ---------------------------------------------------------------------------------------------------------------
; Called by the startup as the demo ends (a5 = CUSTOM), while our interrupt is still in place.

CopperExit:
        ifeq    OS_FRIENDLY
        move.w  #INTF_COPER,INTENA(a5)  ; no more copper interrupts
        move.l  VectorBase,a0
        move.l  NextLevel3,$6c(a0)      ; the startup's handler back in the vector
        else
        move.l  ExecBase.w,a6
        moveq   #INTB_COPER,d0
        lea     CopperServer,a1
        jsr     _LVORemIntServer(a6)    ; (from Kickstart 2 on, this turns the copper interrupt off too)
        endc
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the band moves a line, by rewriting the line numbers in its copper waits; and its colour moves
; on, for the interrupt to use.

Frame:
        move.w  BandLine,d0
        add.w   BandStep,d0
        cmp.w   #BANDTOP,d0
        ble.s   .turn
        cmp.w   #BANDBOTTOM-BANDLINES,d0
        blt.s   .move
.turn:
        neg.w   BandStep
.move:
        move.w  d0,BandLine
        move.b  d0,CopperBandTop
        add.w   #BANDLINES,d0
        move.b  d0,CopperBandBottom

        ; The band's colour, read by the interrupt. One word, written by one instruction: the interrupt can
        ; come before it or after it, but never half way through, so it always reads a whole colour.
        move.w  BandHue,d0
        addq.w  #1,d0
        and.w   #127,d0                 ; a new colour every 8 frames, from 16
        move.w  d0,BandHue
        lsr.w   #3-1,d0
        and.w   #15*2,d0
        lea     Hues,a0
        move.w  (a0,d0.w),d0
        move.w  d0,BandColour
        move.w  d0,CopperBandColour+2
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the startup's interrupt: the first copper interrupt of the frame is next.

VBlank:
        clr.w   CopperCount
        rts

; ---------------------------------------------------------------------------------------------------------------
; The copper interrupt's work, the same both ways. a0 = CUSTOM; changes d0.
;
; It comes twice a frame, at the band's top and bottom: CopperCount says which, and goes back to 0 in VBlank.
; Both are level 3 interrupts, so neither can interrupt the other, and they can share CopperCount safely.

CopperWork:
        move.w  CopperCount,d0
        bne.s   .bottom
        move.w  BandColour,COLOR00(a0)  ; the band's top: its colour
        bra.s   .next
.bottom:
        move.w  #$012,COLOR00(a0)       ; its bottom: the background again
.next:
        addq.w  #1,CopperCount
        rts

        ifeq    OS_FRIENDLY
; ---------------------------------------------------------------------------------------------------------------
; Taking the machine: our level 3 handler, in front of the startup's. All three level 3 interrupts (copper,
; frame, blitter) come here, so first find out whether it's ours.

CopperLevel3:
        btst    #INTB_COPER,CUSTOM+INTREQR+1
        beq.s   .notours
        ; Acknowledged as soon as we know it's ours, so that a new request while we work isn't lost; and
        ; twice, because on a fast processor one write may not have reached the chips by the time the
        ; interrupt returns, and it would come straight back.
        move.w  #INTF_COPER,CUSTOM+INTREQ
        move.w  #INTF_COPER,CUSTOM+INTREQ
        movem.l d0/a0-a1,-(sp)
        lea     CUSTOM,a0
        bsr     CopperWork
        movem.l (sp)+,d0/a0-a1
.notours:
        ; On to the startup's handler, as if the processor had gone straight there: its address is pushed and
        ; "returned" to, with the stack just as the interrupt left it.
        move.l  NextLevel3,-(sp)
        rts

        else
; ---------------------------------------------------------------------------------------------------------------
; Keeping the system running: our server on the copper chain. exec calls it as a subroutine with a1 = is_Data;
; it may change d0, d1, a0, a1, a5 and a6, and must keep the rest. exec acknowledges the interrupt itself.

CopperServerCode:
        lea     CUSTOM,a0
        bsr     CopperWork
        moveq   #0,d0                   ; Z set: let any other servers on the chain run too
        rts

        endc

        section data,data

NextLevel3:     dc.l    0
BandLine:       dc.w    BANDTOP+1
BandStep:       dc.w    1
BandHue:        dc.w    0
BandColour:     dc.w    $f80            ; written by Frame, read by the interrupt
CopperCount:    dc.w    0               ; which copper interrupt of the frame comes next

Hues:           dc.w    $f00,$f40,$f80,$fc0,$ff0,$cf0,$8f0,$4f0
                dc.w    $0f4,$0f8,$0fc,$0ff,$0cf,$08f,$04f,$40f

        ifne    OS_FRIENDLY
CopperServer:
        dc.l    0,0                     ; the node's links
        dc.b    NT_INTERRUPT,0          ; its type, and its priority on the chain
        dc.l    CopperServerName        ; a name, so anyone looking at the chain can see whose it is
        dc.l    0,CopperServerCode      ; is_Data, is_Code
CopperServerName:
        dc.b    "How To Code v8 copper interrupt",0
        even
        endc

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the ruler, in chip RAM, where the chips can read them.

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
        dc.w    BPL1MOD,-ROWBYTES       ; every line shows the same row: the ruler
        dc.w    COLOR00,$0012
        dc.w    COLOR01,$0468
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

        ; The band's top: the copper waits for INTHPOS on the line, and raises the copper interrupt by writing
        ; its bit, with SETCLR, to INTREQ. Around that it makes the background white for two moves (16 pixels),
        ; to mark the place. The band itself starts when the processor gets round to it.
CopperBandTop:
        dc.w    (BANDTOP+1)<<8!INTHPOS,$fffe
        dc.w    COLOR00,$0fff
        dc.w    INTREQ,INTF_SETCLR!INTF_COPER
        dc.w    COLOR00,$0012
        ; Its bottom: the same, with the band's colour after the mark (Frame writes it here), until the
        ; processor puts the background back.
CopperBandBottom:
        dc.w    (BANDTOP+1+BANDLINES)<<8!INTHPOS,$fffe
        dc.w    COLOR00,$0fff
        dc.w    INTREQ,INTF_SETCLR!INTF_COPER
CopperBandColour:
        dc.w    COLOR00,$0f80
        dc.w    $ffff,$fffe             ; the end: wait for a line that never comes

NoSprite:
        dc.l    0,0

Ruler:
        rept    ROWBYTES/2
        dc.w    $8000
        endr
