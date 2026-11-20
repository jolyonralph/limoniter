; ch11_keyboard_joystick.s - the keyboard, the mouse and the joystick, read and shown as they happen: a cell
; for each of the 128 raw key codes lights while its key is held, the joystick's directions and buttons light
; theirs, a sprite follows the mouse, and the registers are shown in hexadecimal. Left mouse button or Esc to
; leave.
;
; How To Code v8, chapter 11. CC0: use it for anything.
;
; Both ways. Taking the machine (OS_FRIENDLY 0), the keyboard is read by its own level 2 interrupt, straight
; from the CIA, with the handshake timed properly. With the system running (OS_FRIENDLY 1), the keys come from
; an input handler of its own, and on Kickstart 2 and later keymap.library turns each into the character it
; types. Either way the keys go onto a queue, for Frame to take off.
;
; The numbers, top to bottom: JOY0DAT (the mouse's counters), JOY1DAT (the joystick), POTGOR (the second
; buttons), the last raw key code, and the character that key types (with the system running; -- if none).
;
; $VER: ch11_keyboard_joystick 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8

KEYQUEUE        equ     16              ; raw codes the queue can hold for Frame (a power of two)
KEY_ESC         equ     $45             ; Esc's raw key code
KEYTOP          equ     16              ; where the grid of keys starts, in lines from the top of the screen
CELLLINES       equ     12              ; each cell's height; cells are KEYPITCH lines apart
KEYPITCH        equ     14
POINTERLINES    equ     11              ; the mouse pointer sprite's height
NUMBERX         equ     22              ; the numbers: their column in bytes, top line, and lines apart
NUMBERY         equ     146
NUMBERPITCH     equ     12

; Not in system.i: keymap.library and potgo.resource.
_LVOMapRawKey   equ     -42
_LVOAllocPotBits equ    -6              ; potgo.resource
_LVOFreePotBits equ     -12
_LVOWritePotgo  equ     -18
OUTRY           equ     $8000           ; POTGO: the joystick port's pin 9 an output...
DATRY           equ     $4000           ; ...driven high (and in POTGOR, the pin's level)
DATLY           equ     $0400           ; POTGOR: the mouse port's pin 9 (the right mouse button)
ie_Qualifier    equ     8
ie_SIZEOF       equ     22
IEQUALIFIERB_REPEAT equ 9               ; in ie_Qualifier: a key held down, repeating

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; The two bitplanes into the copper list.
        lea     CopperBitplanes,a0
        move.l  #Plane0,d0
        bsr     SetPointer
        move.l  #Plane1,d0
        bsr     SetPointer

        ; Sprite 0 is the mouse pointer; the other seven are empty.
        lea     CopperSprites,a0
        move.l  #Pointer,d0
        bsr     SetPointer
        moveq   #7-1,d1
.sprite:
        move.l  #NoSprite,d0
        bsr     SetPointer
        dbf     d1,.sprite

        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2 ; NTSC: 200 lines
.pal:

        ; Clear both bitplanes (the processor's fine for once), then draw every cell, unlit, in bitplane 0.
        lea     Plane1,a0               ; the first of the two
        move.w  #ROWBYTES*HEIGHT*2/4-1,d0
.clear:
        clr.l   (a0)+
        dbf     d0,.clear
        moveq   #0,d7
.key:
        move.w  d7,d0
        bsr     KeyCell
        lea     Plane0-Plane1(a0),a0
        moveq   #-1,d0
        bsr     DrawCell
        addq.w  #1,d7
        cmp.w   #128,d7
        bne.s   .key
        lea     ControlCells,a2
        moveq   #CONTROLS-1,d7
.control:
        move.l  (a2)+,a0
        lea     Plane0-Plane1(a0),a0
        moveq   #-1,d0
        bsr     DrawCell
        dbf     d7,.control

        ; The mouse's counters as they are now, so the first frame's movement is from here.
        move.w  JOY0DAT(a5),OldMouse

        ; Whether sprites were on, to leave them as they were.
        move.w  DMACONR(a5),d0
        and.w   #DMAF_SPRITE,d0
        move.w  d0,OldSprites

        ; A joystick's second button is on pin 9, one of the "pot" pins, which only reads as a button with a
        ; pull-up: the pin set as an output, driven high (the Hardware Reference Manual's way). The system does
        ; this for the mouse port's pin 9 (the right button), but not always for the joystick port's (Kickstart
        ; 1.3 doesn't). POTGO is shared and can't be read back, so it's asked for and set through
        ; potgo.resource, which keeps the system's copy right - taking the machine or not.
        move.l  ExecBase.w,a6
        lea     PotgoName(pc),a1
        jsr     _LVOOpenResource(a6)
        move.l  d0,PotgoBase
        beq.s   .nopotgo
        move.l  d0,a6
        move.l  #OUTRY!DATRY,d0
        jsr     _LVOAllocPotBits(a6)
        move.w  d0,PotBits              ; what we were given (0 if someone else has them)
        and.w   #DATRY,d0
        beq.s   .nopotgo
        move.l  #OUTRY!DATRY,d0
        move.l  d0,d1
        jsr     _LVOWritePotgo(a6)
.nopotgo:
        lea     CUSTOM,a5

        ifeq    OS_FRIENDLY
        ; Taking the machine, the keyboard is ours to read. Which CIA-A interrupts the system has switched on
        ; (cia.resource keeps the mask; the CIA's own can't be read back), so the ones it misses while the demo
        ; runs can be given to it at the end.
        move.l  ExecBase.w,a6
        lea     CiaaName(pc),a1
        jsr     _LVOOpenResource(a6)
        move.l  d0,CiaaBase
        beq.s   .nocia
        move.l  d0,a6
        moveq   #0,d0                   ; AbleICR(0): change nothing, return the mask
        jsr     _LVOAbleICR(a6)
        move.b  d0,SystemICRMask
.nocia:
        ; Our level 2 interrupt, at the vector base the startup found (not at address $68: on a 68010 or later
        ; the system may have moved the vectors). The CIA's serial port interrupt is already switched on in
        ; the CIA, by the system's keyboard.device; a trackmo with no system would write $88 to CIA-A's ICR.
        lea     CUSTOM,a5
        move.l  VectorBase,a0
        move.l  $68(a0),OldLevel2
        move.l  #Level2,$68(a0)
        move.w  #INTF_SETCLR!INTF_PORTS,INTENA(a5)
        else
        ; With the system running, keys come through input.device. The startup has an input handler (at
        ; priority 100) that keeps only the last key, and stops keys reaching Workbench; this one goes ahead of
        ; it (priority 101) and queues every key. It's added with the startup's own request to input.device,
        ; which is open already.
        move.l  ExecBase.w,a6
        lea     InputRequest,a1
        move.w  #IND_ADDHANDLER,IO_COMMAND(a1)
        move.l  #KeyHandler,IO_DATA(a1)
        jsr     _LVODoIO(a6)
        ; keymap.library turns raw keys into characters. It's Kickstart 2's, so on 1.3 this fails and the
        ; character is shown as --.
        lea     KeymapName(pc),a1
        moveq   #36,d0
        jsr     _LVOOpenLibrary(a6)
        move.l  d0,KeymapBase
        endc

        move.l  #InputExit,ExitHook     ; to take it all out again as the demo ends
        rts

; a0 = two words of a copper move to fill in with the address in d0 (high word, then low); a0 moves on past them.
SetPointer:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        rts

; ---------------------------------------------------------------------------------------------------------------
; Called by the startup first as the demo ends, with everything still running: take out what Init put in.

InputExit:
        ifeq    OS_FRIENDLY
        ; No more level 2 interrupts to our code, and the system's own vector back.
        move.w  #INTF_PORTS,INTENA(a5)
        move.l  VectorBase,a0
        move.l  OldLevel2,$68(a0)
        ; Reading CIA-A's ICR clears every event in it, not only the keyboard's: any the system was waiting
        ; for (its timers, say) are handed back to cia.resource with SetICR, which passes them on to the
        ; system's handlers. Not the keyboard's: that byte was ours, already read and handshaken.
        move.l  CiaaBase,d0
        beq.s   .nocia
        move.l  d0,a6
        move.b  MissedICR,d0
        and.b   SystemICRMask,d0
        and.w   #$1f&~(1<<CIAICRB_SP),d0
        beq.s   .nocia
        or.w    #$80,d0                 ; bit 7 set: cause them
        jsr     _LVOSetICR(a6)
.nocia:
        else
        move.l  ExecBase.w,a6
        lea     InputRequest,a1
        move.w  #IND_REMHANDLER,IO_COMMAND(a1)
        move.l  #KeyHandler,IO_DATA(a1)
        jsr     _LVODoIO(a6)
        move.l  KeymapBase,d0
        beq.s   .nokeymap
        move.l  d0,a1
        move.l  ExecBase.w,a6
        jsr     _LVOCloseLibrary(a6)
.nokeymap:
        endc
        ; The joystick's pin 9 back to an input, and given back.
        move.l  PotgoBase,d0
        beq.s   .nopotgo
        move.l  d0,a6
        move.w  PotBits,d0
        beq.s   .nopotgo
        moveq   #0,d0
        move.l  #OUTRY!DATRY,d1
        jsr     _LVOWritePotgo(a6)
        move.w  PotBits,d0
        jsr     _LVOFreePotBits(a6)
.nopotgo:
        ; Sprites off again if they were off (with the system running they're on, for the mouse pointer).
        lea     CUSTOM,a5
        tst.w   OldSprites
        bne.s   .sprites
        move.w  #DMAF_SPRITE,DMACON(a5)
.sprites:
        rts

        ifeq    OS_FRIENDLY
; ---------------------------------------------------------------------------------------------------------------
; The level 2 interrupt, taking the machine: CIA-A (and anything on the expansion bus that uses level 2, which
; the system's own handler would see to; with the machine taken, nothing should be busy there).
;
; A byte from the keyboard: read it from the serial data register, then the handshake - the serial port made
; an output for at least 85 microseconds, which pulls the keyboard's data line low and tells it the byte has
; arrived. Until it sees that, the keyboard sends nothing more. The time is measured with the raster, not a
; loop: a loop that's long enough on a 68000 is far too short on a 68040, and the keyboard then misses the
; handshake (and resends, or gives up: the keys "stick").

Level2:
        movem.l d0-d2/a0-a1,-(sp)
        ; Which of CIA-A's events this is. Reading the ICR clears them all at once, so any but the keyboard's
        ; are noted for the system (see InputExit).
        move.b  CIAA+CIAICR,d0
        move.b  d0,d1
        and.b   #$1f&~(1<<CIAICRB_SP),d1
        or.b    d1,MissedICR
        btst    #CIAICRB_SP,d0
        beq.s   .done

        move.b  CIAA+CIASDR,d0          ; the byte, as it came
        bset    #CIACRAB_SPMODE,CIAA+CIACRA ; the handshake starts: KDAT low
        move.b  CUSTOM+VHPOSR,d2        ; the line it started on

        ; The byte is sent last bit first and inverted (a 1 is a low level): put it right.
        not.b   d0
        ror.b   #1,d0
        bsr     QueueKey
        ; The rest of the 85 microseconds: until three new lines have started, so at least two whole lines
        ; have gone by since the handshake began (64 microseconds each, 63.5 on NTSC): at least 127
        ; microseconds, on any processor.
        moveq   #3-1,d1
.line:
        cmp.b   CUSTOM+VHPOSR,d2
        beq.s   .line
        move.b  CUSTOM+VHPOSR,d2
        dbf     d1,.line
        bclr    #CIACRAB_SPMODE,CIAA+CIACRA ; the handshake ends: KDAT back to an input, ready for the next byte

.done:
        ; Acknowledged after the ICR has been read (which lets go of the CIA's interrupt line), and twice, for
        ; fast processors, as in the startup's level 3 interrupt.
        move.w  #INTF_PORTS,CUSTOM+INTREQ
        move.w  #INTF_PORTS,CUSTOM+INTREQ
        movem.l (sp)+,d0-d2/a0-a1
        rte

        else
; ---------------------------------------------------------------------------------------------------------------
; The input handler, with the system running: called by input.device (in its own task) with a0 = a list of
; input events, before the startup's handler and Intuition see them. Each key is queued, and the list goes on
; as it came (d0). Keys held down repeat, as events with the REPEAT qualifier: those aren't new presses.

KeyHandlerCode:
        move.l  a0,-(sp)
.event:
        cmp.b   #IECLASS_RAWKEY,ie_Class(a0)
        bne.s   .next
        btst    #IEQUALIFIERB_REPEAT-8,ie_Qualifier(a0)
        bne.s   .next
        move.b  ie_Code+1(a0),d0
        bsr     QueueKey
.next:
        move.l  (a0),d0
        move.l  d0,a0
        bne.s   .event
        move.l  (sp)+,d0
        rts
        endc

; ---------------------------------------------------------------------------------------------------------------
; d0.b = a raw key code, onto the queue for Frame, unless it's full (then it's lost). The queue is 16 bytes
; with two counters: KeyHead, written only here, and KeyTail, written only by Frame, so the two never need to
; stop each other. Changes d1 and a1.

QueueKey:
        move.b  KeyHead,d1
        sub.b   KeyTail,d1
        cmp.b   #KEYQUEUE,d1
        bhs.s   .full
        moveq   #KEYQUEUE-1,d1
        and.b   KeyHead,d1
        lea     KeyQueue,a1
        move.b  d0,(a1,d1.w)
        addq.b  #1,KeyHead
.full:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame.

Frame:
        ; Sprites on, now that our copper list (with our sprite pointers) is running.
        tst.b   SpritesOn
        bne.s   .on
        move.w  #DMAF_SETCLR!DMAF_SPRITE,DMACON(a5)
        st      SpritesOn
.on:
        bsr     ReadKeys
        bsr     ReadMouse
        bsr     ReadControls
        bsr     DrawKeys
        bsr     DrawControls
        bsr     DrawNumbers
        rts

; ---------------------------------------------------------------------------------------------------------------
; The keys that have come since the last frame.

ReadKeys:
.next:
        move.b  KeyTail,d1
        cmp.b   KeyHead,d1
        beq.s   .done
        and.w   #KEYQUEUE-1,d1
        lea     KeyQueue,a0
        moveq   #0,d0
        move.b  (a0,d1.w),d0
        addq.b  #1,KeyTail
        bsr     KeyEvent
        ifne    OS_FRIENDLY
        bsr     MapKey
        endc
        bra.s   .next
.done:
        rts

; d0.w = a raw key code: bits 0-6 which key, bit 7 set when it's let go.
KeyEvent:
        move.b  d0,LastCode
        moveq   #$7f,d1
        and.w   d0,d1
        cmp.w   #$68,d1                 ; $68-$7f aren't keys: $78 (reset warning) and $f9-$fe are the
        bhs.s   .out                    ; keyboard's own messages
        lea     KeyDown,a0
        tst.b   d0
        bmi.s   .up
        st      (a0,d1.w)
        cmp.w   #KEY_ESC,d1
        bne.s   .out
        st      ExitNow
        rts
.up:
        sf      (a0,d1.w)
.out:
        rts

        ifne    OS_FRIENDLY
; With the system running: the character a key makes, by the user's keymap (Kickstart 2 and later). d0.w = the
; raw code. The event is made up here with no qualifiers (so no Shift); a program that wanted Shift and Alt would
; keep the event's ie_Qualifier too.
MapKey:
        tst.b   d0
        bmi.s   .out                    ; only as keys go down
        move.w  #-1,MappedChar
        move.l  KeymapBase,d1
        beq.s   .out
        lea     RawKeyEvent,a0
        move.b  #IECLASS_RAWKEY,ie_Class(a0)
        move.w  d0,ie_Code(a0)
        clr.w   ie_Qualifier(a0)
        lea     MapBuffer,a1
        moveq   #8,d1
        sub.l   a2,a2                   ; the default keymap
        move.l  KeymapBase,a6
        jsr     _LVOMapRawKey(a6)
        tst.w   d0
        ble.s   .out                    ; no character (Shift, say), or more than would fit
        moveq   #0,d0
        move.b  MapBuffer,d0
        move.w  d0,MappedChar
.out:
        rts
        endc

; ---------------------------------------------------------------------------------------------------------------
; The mouse: JOY0DAT holds two 8-bit counters, vertical in the high byte, horizontal in the low, which count
; up and down as it moves and wrap around. How far it moved is this frame's count minus the last, as a signed
; byte: right as long as it moves less than 128 counts a frame.

ReadMouse:
        move.w  JOY0DAT(a5),d0
        move.w  OldMouse,d1
        move.w  d0,OldMouse
        move.b  d0,d2                   ; horizontal: the low bytes
        sub.b   d1,d2
        ext.w   d2
        add.w   PointerX,d2
        bpl.s   .xmin
        moveq   #0,d2
.xmin:
        cmp.w   #WIDTH-1,d2
        ble.s   .xmax
        move.w  #WIDTH-1,d2
.xmax:
        move.w  d2,PointerX
        lsr.w   #8,d0                   ; vertical: the high bytes
        lsr.w   #8,d1
        sub.b   d1,d0
        ext.w   d0
        add.w   PointerY,d0
        bpl.s   .ymin
        moveq   #0,d0
.ymin:
        cmp.w   #HEIGHT-1,d0
        ble.s   .ymax
        move.w  #HEIGHT-1,d0
.ymax:
        move.w  d0,PointerY

        ; The sprite's control words: its first line (VSTART), the line after its last (VSTOP), and its left
        ; edge (HSTART), each nine bits, spread over the two words as the hardware wants them.
        add.w   #$2c,d0                 ; VSTART: the display window's top line is $2c
        move.w  d0,d1
        add.w   #POINTERLINES,d1        ; VSTOP
        move.w  PointerX,d2
        add.w   #$80,d2                 ; HSTART: the window's left edge is $81
        moveq   #0,d3                   ; SPRxPOS: VSTART bits 7-0, then HSTART bits 8-1
        move.b  d0,d3
        lsl.w   #8,d3
        move.w  d2,d4
        lsr.w   #1,d4
        move.b  d4,d3
        moveq   #0,d4                   ; SPRxCTL: VSTOP bits 7-0, then VSTART bit 8, VSTOP bit 8, HSTART bit 0
        move.b  d1,d4
        lsl.w   #8,d4
        btst    #8,d0
        beq.s   .v8
        bset    #2,d4
.v8:
        btst    #8,d1
        beq.s   .e8
        bset    #1,d4
.e8:
        btst    #0,d2
        beq.s   .h0
        bset    #0,d4
.h0:
        move.w  d3,Pointer
        move.w  d4,Pointer+2
        rts

; ---------------------------------------------------------------------------------------------------------------
; The joystick, and the buttons. JOY1DAT has the joystick's four switches in the same bits the mouse's counters
; use: right is bit 1 and left bit 9, but down is bit 1 XOR bit 0, and up bit 9 XOR bit 8.

ReadControls:
        lea     Controls,a0
        move.w  JOY1DAT(a5),d0
        move.w  d0,d1
        lsr.w   #1,d1
        eor.w   d0,d1                   ; now bit 8 = up, bit 0 = down
        btst    #8,d1
        sne     (a0)+                   ; up
        btst    #0,d1
        sne     (a0)+                   ; down
        btst    #9,d0
        sne     (a0)+                   ; left
        btst    #1,d0
        sne     (a0)+                   ; right
        ; The fire buttons are on CIA-A's port A: bit 7 the joystick's, bit 6 the mouse's left button; 0 while
        ; pressed.
        btst    #CIAB_GAMEPORT1,CIAA+CIAPRA
        seq     (a0)+                   ; fire
        ; Second buttons (the mouse's right, a joystick's second) are on the "pot" pins, read in POTGOR: bit 14
        ; for the joystick port, bit 10 for the mouse's; 0 while pressed. POTGO, which sets them up, is the
        ; system's (see Init).
        move.w  POTGOR(a5),d2
        and.w   #DATRY,d2
        seq     (a0)+                   ; second fire
        btst    #CIAB_GAMEPORT0,CIAA+CIAPRA
        seq     (a0)+                   ; left mouse button
        move.w  POTGOR(a5),d2
        and.w   #DATLY,d2
        seq     (a0)+                   ; right mouse button
        rts

; ---------------------------------------------------------------------------------------------------------------
; Drawing. A cell lights by being filled in bitplane 1 as well as bitplane 0 (colour 3); only the cells that
; have changed are drawn.

DrawKeys:
        lea     KeyDown,a2
        lea     KeyDrawn,a3
        moveq   #0,d7
.key:
        move.b  (a2,d7.w),d0
        cmp.b   (a3,d7.w),d0
        beq.s   .same
        move.b  d0,(a3,d7.w)
        move.b  d0,d6
        move.w  d7,d0
        bsr     KeyCell
        move.b  d6,d0
        bsr     DrawCell
.same:
        addq.w  #1,d7
        cmp.w   #128,d7
        bne.s   .key
        rts

DrawControls:
        lea     Controls,a2
        lea     ControlCells,a3
        moveq   #CONTROLS-1,d7
.control:
        move.l  (a3)+,a0
        move.b  (a2)+,d0
        bsr     DrawCell
        dbf     d7,.control
        rts

; d0.w = a raw key code: a0 = its cell, in bitplane 1. The cells are 16 to a row, in code order.
KeyCell:
        moveq   #15,d1
        and.w   d0,d1
        add.w   d1,d1                   ; a word across
        lsr.w   #4,d0
        mulu    #KEYPITCH*ROWBYTES,d0
        lea     Plane1+KEYTOP*ROWBYTES+4,a0
        add.w   d1,a0
        add.l   d0,a0
        rts

; a0 = a cell; d0.b = 0 to clear it, anything else to fill it.
DrawCell:
        moveq   #0,d1
        tst.b   d0
        beq.s   .fill
        move.w  #$7ffe,d1
.fill:
        moveq   #CELLLINES-1,d2
.line:
        move.w  d1,(a0)
        lea     ROWBYTES(a0),a0
        dbf     d2,.line
        rts

DrawNumbers:
        lea     Plane1+NUMBERY*ROWBYTES+NUMBERX,a0
        move.w  JOY0DAT(a5),d0
        moveq   #4,d1
        bsr     DrawHex
        lea     Plane1+(NUMBERY+NUMBERPITCH)*ROWBYTES+NUMBERX,a0
        move.w  JOY1DAT(a5),d0
        moveq   #4,d1
        bsr     DrawHex
        lea     Plane1+(NUMBERY+NUMBERPITCH*2)*ROWBYTES+NUMBERX,a0
        move.w  POTGOR(a5),d0
        moveq   #4,d1
        bsr     DrawHex
        lea     Plane1+(NUMBERY+NUMBERPITCH*3)*ROWBYTES+NUMBERX,a0
        move.b  LastCode,d0
        moveq   #2,d1
        bsr     DrawHex
        lea     Plane1+(NUMBERY+NUMBERPITCH*4)*ROWBYTES+NUMBERX,a0
        move.w  MappedChar,d0
        bmi.s   .none
        moveq   #2,d1
        bra     DrawHex
.none:
        moveq   #16,d0                  ; the dash
        bsr     DrawGlyph
        moveq   #16,d0
        bra     DrawGlyph

; d0 = a number, d1.w = how many hexadecimal digits of it (its lowest), a0 = where, in a bitplane.
DrawHex:
        moveq   #8,d2
        sub.w   d1,d2
        lsl.w   #2,d2
        lsl.l   d2,d0                   ; the first digit wanted to the top four bits
        subq.w  #1,d1
.digit:
        rol.l   #4,d0
        move.l  d0,-(sp)
        and.w   #15,d0
        bsr     DrawGlyph
        move.l  (sp)+,d0
        dbf     d1,.digit
        rts

; d0.w = which glyph (0-15 the digits, 16 a dash), a0 = where; a0 moves on to the next character.
DrawGlyph:
        lsl.w   #3,d0
        lea     Font,a1
        add.w   d0,a1
        move.b  (a1)+,(a0)
        move.b  (a1)+,ROWBYTES(a0)
        move.b  (a1)+,ROWBYTES*2(a0)
        move.b  (a1)+,ROWBYTES*3(a0)
        move.b  (a1)+,ROWBYTES*4(a0)
        move.b  (a1)+,ROWBYTES*5(a0)
        move.b  (a1)+,ROWBYTES*6(a0)
        move.b  (a1)+,ROWBYTES*7(a0)
        addq.l  #1,a0
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do here.

VBlank:
        rts

CiaaName:       dc.b    "ciaa.resource",0
PotgoName:      dc.b    "potgo.resource",0
KeymapName:     dc.b    "keymap.library",0
        even

        section data,data

; Where each control's cell is, in bitplane 1, in the order ReadControls fills in Controls.
CELL            macro                   ; x in bytes, y in lines
        dc.l    Plane1+(\2)*ROWBYTES+(\1)
        endm
ControlCells:
        CELL    6,144                   ; up
        CELL    6,172                   ; down
        CELL    4,158                   ; left
        CELL    8,158                   ; right
        CELL    6,158                   ; fire, in the middle
        CELL    12,158                  ; second fire
        CELL    16,144                  ; left mouse button
        CELL    18,144                  ; right mouse button
CONTROLS        equ     (*-ControlCells)/4

PointerX:       dc.w    WIDTH/2
PointerY:       dc.w    HEIGHT/2
MappedChar:     dc.w    -1
OldMouse:       dc.w    0
OldSprites:     dc.w    0
PotBits:        dc.w    0
CiaaBase:       dc.l    0
PotgoBase:      dc.l    0
KeymapBase:     dc.l    0
OldLevel2:      dc.l    0
KeyHead:        dc.b    0               ; written only by QueueKey
KeyTail:        dc.b    0               ; written only by Frame
LastCode:       dc.b    0
MissedICR:      dc.b    0
SystemICRMask:  dc.b    0
SpritesOn:      dc.b    0
        even

        ifne    OS_FRIENDLY
KeyHandler:
        dc.l    0,0                     ; the node's links
        dc.b    NT_INTERRUPT,101        ; ahead of the startup's input handler (100) and Intuition (50)
        dc.l    KeyHandlerName
        dc.l    0,KeyHandlerCode        ; is_Data, is_Code
KeyHandlerName: dc.b    "How To Code v8 keys",0
        even
        endc

; 8 by 8 digits 0-9 and A-F, then a dash.
Font:
        dc.b    %01111100,%11000110,%11001110,%11010110,%11100110,%11000110,%01111100,0
        dc.b    %00011000,%00111000,%00011000,%00011000,%00011000,%00011000,%01111110,0
        dc.b    %01111100,%11000110,%00000110,%00011100,%01110000,%11000000,%11111110,0
        dc.b    %01111100,%11000110,%00000110,%00111100,%00000110,%11000110,%01111100,0
        dc.b    %00011100,%00111100,%01101100,%11001100,%11111110,%00001100,%00001100,0
        dc.b    %11111110,%11000000,%11111100,%00000110,%00000110,%11000110,%01111100,0
        dc.b    %00111100,%01100000,%11000000,%11111100,%11000110,%11000110,%01111100,0
        dc.b    %11111110,%00000110,%00001100,%00011000,%00110000,%00110000,%00110000,0
        dc.b    %01111100,%11000110,%11000110,%01111100,%11000110,%11000110,%01111100,0
        dc.b    %01111100,%11000110,%11000110,%01111110,%00000110,%00001100,%01111000,0
        dc.b    %00111000,%01101100,%11000110,%11000110,%11111110,%11000110,%11000110,0
        dc.b    %11111100,%11000110,%11000110,%11111100,%11000110,%11000110,%11111100,0
        dc.b    %01111100,%11000110,%11000000,%11000000,%11000000,%11000110,%01111100,0
        dc.b    %11111000,%11001100,%11000110,%11000110,%11000110,%11001100,%11111000,0
        dc.b    %11111110,%11000000,%11000000,%11111000,%11000000,%11000000,%11111110,0
        dc.b    %11111110,%11000000,%11000000,%11111000,%11000000,%11000000,%11000000,0
        dc.b    0,0,0,%01111100,0,0,0,0

        section bss,bss

KeyQueue:       ds.b    KEYQUEUE
KeyDown:        ds.b    128             ; for each raw key code, not 0 while its key is held
KeyDrawn:       ds.b    128             ; what's on the screen
Controls:       ds.b    CONTROLS
        even
RawKeyEvent:    ds.b    ie_SIZEOF
MapBuffer:      ds.b    8

; ---------------------------------------------------------------------------------------------------------------
; The copper list, the sprites and the screen, in chip RAM.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$2200           ; two bitplanes, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0024           ; sprites in front of the bitplanes
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0112           ; background
        dc.w    COLOR01,$0347           ; a cell
        dc.w    COLOR02,$0eee           ; the numbers
        dc.w    COLOR03,$0fc3           ; a lit cell
        dc.w    COLOR17,$0fff           ; the pointer
        dc.w    COLOR18,$0000
        dc.w    COLOR19,$0f80
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
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

; An arrow: two words a line (the colour's low bit, then its high bit), between the control words and the end.
Pointer:
        dc.w    0,0
        dc.w    %1100000000000000,%0100000000000000
        dc.w    %0111000000000000,%1011000000000000
        dc.w    %0011110000000000,%0100110000000000
        dc.w    %0011111100000000,%0100001100000000
        dc.w    %0001111111000000,%0010000011000000
        dc.w    %0001111110000000,%0010000100000000
        dc.w    %0000111100000000,%0001001000000000
        dc.w    %0000111110000000,%0001010100000000
        dc.w    %0000010111000000,%0000101010000000
        dc.w    %0000010011100000,%0000000101000000
        dc.w    %0000000001000000,%0000000000000000
        dc.w    0,0

NoSprite:
        dc.l    0,0

        section screen,bss_c

Plane1:         ds.b    ROWBYTES*HEIGHT
Plane0:         ds.b    ROWBYTES*HEIGHT
