; =============================================================================
; happybat.asm - the machine-code half of Happy Bat, for OpticOS v1.0.1
; Assemble with pasmo:   pasmo --bin happybat.asm hbcode.bin
;
; Written from scratch for OpticOS (clean room): no Sinclair ROM code is used
; or called.  The code touches only the screen, the keyboard/speaker port and
; its own variables, and reads the frame counter FRAMES (23672), whose
; address OpticOS keeps.  It does not use IX, IY or the alternate registers,
; so the OpticBASIC interpreter's state is left alone.  It needs interrupts on
; (they are, under OpticBASIC) because it waits for frames with HALT.
;
; OpticBASIC's CALL and USR take no register arguments, so BASIC POKEs the
; parameter block below, and USR 60003 hands back the result of a step in BC.
;
; Screen layout (pixel lines from the top of the screen):
;   lines   0- 15  character rows 0-1: the score line, printed by BASIC
;   lines  16-175  the playfield, 160 lines: sky, pipes and the bat
;   lines 176-183  character row 22: the ground, which scrolls
;   lines 184-191  character row 23: help text, printed by BASIC
; Horizontal positions are counted in nibbles (4 pixels = one character at
; 64 columns): the screen is 64 nibbles wide and everything moves one nibble
; left per step.
; =============================================================================

PF_TOP   equ 16             ; first screen line of the playfield
PF_H     equ 160            ; playfield height in lines
GR_TOP   equ 176            ; first line of the ground
SC_END   equ 184            ; one past the ground's last line
YMAX     equ 152            ; lowest top line for the 8-line bat
BAT_COL  equ 8              ; the bat's byte column: x = 64-79
BAT_NIB  equ 16             ; ... = nibble 16
NPIPE    equ 4              ; pipe slots (pipes 24 nibbles apart: at most 4)
FREE     equ $80            ; x value marking a free slot
A_SKY    equ $4C            ; bright, blue paper, green ink (pipes)
A_BAT    equ $4E            ; bright, blue paper, yellow ink (the bat)
A_DEAD   equ $4A            ; bright, blue paper, red ink (a dead bat)
A_GROUND equ $66            ; bright, green paper, yellow ink
A_BASE   equ $60            ; bright, green paper, black ink (row 23)
ATTRS    equ $5800          ; start of the attribute map
FRAMES   equ 23672          ; OpticOS frame counter (low byte)

        org 60000

; ---- entry points: the addresses BASIC calls ---------------------------------
        jp init             ; 60000  CALL 60000: new playfield
        jp step             ; 60003  USR 60003: one step -> BC = 0, 1 or 2
        jp dead             ; 60006  CALL 60006: colour the bat red

; ---- parameters, POKEd (and PEEKed) by BASIC -------------------------------
baty:   defb 72             ; 60009  bat's top line in the playfield (0-152);
                            ;        BASIC sets it on the title screen and
                            ;        before CALL 60000; in play the code moves it
gaptop: defb 40             ; 60010  first line of the next pipe's gap
gapbot: defb 88             ; 60011  first line below that gap
newpipe: defb 0             ; 60012  POKE 1: start that pipe at the right edge
mode:   defb 1              ; 60013  1 = play; 0 = title (no gravity, no
                            ;        pipes: only the ground scrolls)
sig:    defm "HB"           ; 60014  signature: BASIC checks it to see
                            ;        whether the code is already loaded
grav:   defb 2              ; 60016  gravity, quarter pixels per step per step
flapv:  defb 18             ; 60017  a flap's upward speed, quarter px / step
vmax:   defb 20             ; 60018  fastest fall, quarter pixels per step
pacev:  defb 2              ; 60019  frames per step at least (1 or more)

; ---- internal state -------------------------------------------------------
y16:    defw 288            ; bat height in quarter pixels (0 - 4*YMAX)
v16:    defw 0              ; its speed, quarter pixels per step (+ = down)
pkey:   defb 0              ; 1 if SPACE was down at the last step
oldy:   defb 0              ; where the bat was drawn last
oldspr: defw spr_up         ; and with which wing frame
drawn:  defb 0              ; 1 while the bat is on the screen
tick:   defb 0              ; step counter, for the wing beat
phase:  defb 0              ; ground pattern phase, 0-3
lastf:  defb 0              ; FRAMES at the last step, for pacing
crash:  defb 0              ; set during a step: hit the top or the ground
scored: defb 0              ; set during a step: a pipe got past the bat
ptab:   defs NPIPE*4,FREE   ; pipes: x (nibble of the left edge), gap top,
                            ;        gap bottom, spare
pg1:    defb 0              ; the pipe being drawn: its gap top
pg2:    defb 0              ;   and gap bottom
scol:   defb 0              ;   first byte column written on each line
slen:   defw 0              ;   bytes written on each line (high byte 0)
sbody:  defw 0              ;   where in its body pattern to start
scap:   defw 0              ;   and in its cap pattern
sptr:   defw 0              ; pattern for the lines seg is drawing

; -----------------------------------------------------------------------------
; init: clear the screen below the score line, colour the sky, ground and
; help row, draw the ground, and reset everything: no pipes, no bat on the
; screen, play mode, the bat at height baty and not moving.
; -----------------------------------------------------------------------------
init:   ld c,PF_TOP         ; C = screen line
in_clr: ld a,c
        call ln_addr        ; HL = start of line C
        ld b,32
        xor a
in_cb:  ld (hl),a           ; clear its 32 bytes
        inc l
        djnz in_cb
        inc c
        ld a,c
        cp 192
        jr nz,in_clr
        ld hl,ATTRS+2*32    ; rows 2-21: sky
        ld bc,20*32
        ld a,A_SKY
        call fill
        ld hl,ATTRS+22*32   ; row 22: ground
        ld bc,32
        ld a,A_GROUND
        call fill
        ld hl,ATTRS+23*32   ; row 23: help text
        ld bc,32
        ld a,A_BASE
        call fill
        ld de,ginit         ; draw the ground: two bytes, 16 times a line
        ld c,GR_TOP
in_gr:  ld a,c
        call ln_addr
        ld b,16
in_g2:  ld a,(de)
        ld (hl),a
        inc l
        inc de
        ld a,(de)
        ld (hl),a
        inc l
        dec de
        djnz in_g2
        inc de              ; next ground line's pair
        inc de
        inc c
        ld a,c
        cp SC_END
        jr nz,in_gr
        ld hl,ptab          ; every pipe slot free
        ld bc,NPIPE*4
        ld a,FREE
        call fill
        xor a
        ld (drawn),a        ; no bat on the screen
        ld (tick),a
        ld (phase),a        ; 64 nibbles drawn: the next one is phase 0
        ld (newpipe),a
        ld h,a
        ld l,a
        ld (v16),hl         ; not moving
        inc a
        ld (mode),a         ; play mode
        ld (pkey),a         ; a SPACE still held from before is not a flap
        ld a,(baty)         ; height in quarter pixels = baty * 4
        ld l,a
        add hl,hl
        add hl,hl
        ld (y16),hl
        ld a,(FRAMES)
        ld (lastf),a
        ret

; -----------------------------------------------------------------------------
; step: one step of the game, called with h% = USR 60003.
;  1. wait until pacev frames have passed since the last step, so the game
;     runs at the same speed however much is on the screen;
;  2. take the bat off the screen;
;  3. in play: read SPACE and move the bat (phys), then start, move and draw
;     the pipes (pipes);
;  4. scroll the ground; put the bat back, testing its pixels as it goes.
; Returns BC = 1 if the bat crashed (pipe, ground or top), else 2 if a pipe
; has just got past the bat (a point), else 0.
; -----------------------------------------------------------------------------
step:   call pace
        call bat_off
        xor a
        ld (crash),a
        ld (scored),a
        ld a,(mode)
        or a
        jr z,st_t           ; title: no gravity, no pipes
        call phys
        call pipes
st_t:   call ground
        ld hl,tick
        inc (hl)
        call bat_on         ; C <> 0: the bat touched a pipe
        ld a,c
        or a
        jr z,st_1
        ld (crash),a
st_1:   ld bc,1
        ld a,(crash)
        or a
        ret nz              ; 1: crashed
        inc c
        ld a,(scored)
        or a
        ret nz              ; 2: scored
        ld c,0
        ret                 ; 0: flying on

; -----------------------------------------------------------------------------
; pace: HALT until at least pacev frames have passed since the last call.
; -----------------------------------------------------------------------------
pace:   ld hl,lastf
pa_lp:  ld a,(FRAMES)
        sub (hl)            ; frames since the last step
        ld b,a
        ld a,(pacev)
        dec a
        cp b                ; carry if B >= pacev
        jr c,pa_ok
        halt                ; wait for the next frame
        jr pa_lp
pa_ok:  ld a,(FRAMES)
        ld (hl),a
        ret

; -----------------------------------------------------------------------------
; phys: the bat's flight.  A new press of SPACE (it was up at the last step)
; sets the speed to -flapv and chirps; then gravity adds grav to the speed, up
; to vmax downwards, and the speed is added to the height.  Going above the
; top or below YMAX stops the bat there and sets crash.  baty = height / 4.
; -----------------------------------------------------------------------------
phys:   ld a,$7F            ; keyboard half-row SPACE, SYMBOL, M, N, B
        in a,($FE)
        cpl
        and 1               ; A = 1 while SPACE is held
        ld hl,pkey
        ld b,(hl)
        ld (hl),a
        jr z,ph_g           ; not held
        dec b
        jr z,ph_g           ; held, but it was held last step too
        ld a,(flapv)        ; a flap: speed = -flapv
        neg
        ld l,a
        ld h,$FF
        ld (v16),hl
        call chirp
ph_g:   ld hl,(v16)         ; gravity
        ld a,(grav)
        ld e,a
        ld d,0
        add hl,de
        bit 7,h
        jr nz,ph_v          ; still going up: no limit
        ld a,(vmax)
        ld e,a
        ld a,h
        or a
        jr nz,ph_cl
        ld a,l
        cp e
        jr c,ph_v
ph_cl:  ex de,hl            ; falling too fast: speed = vmax
ph_v:   ld (v16),hl
        ld de,(y16)
        add hl,de           ; new height
        bit 7,h
        jr z,ph_lo
        ld hl,0             ; above the top of the playfield
        jr ph_cr
ph_lo:  push hl
        ld de,YMAX*4+1
        or a
        sbc hl,de
        pop hl
        jr c,ph_ok
        ld hl,YMAX*4        ; on the ground
ph_cr:  ld a,1
        ld (crash),a
ph_ok:  ld (y16),hl
        srl h               ; baty = height / 4
        rr l
        srl h
        rr l
        ld a,l
        ld (baty),a
        ret

; -----------------------------------------------------------------------------
; chirp: a short rising click on the speaker for a flap (about 1 ms).
; Bit 4 of port 254 drives the speaker; bits 0-2 keep the border black.
; -----------------------------------------------------------------------------
chirp:  ld b,24
ch_lp:  ld a,b
        and 1
        rlca
        rlca
        rlca
        rlca
        out ($FE),a
        ld a,b
        add a,8
ch_d:   dec a
        jr nz,ch_d
        djnz ch_lp
        ret

; -----------------------------------------------------------------------------
; pipes: if BASIC asked for one (newpipe), put a new pipe in a free slot at
; x = 64, just off the right edge, with the gap from gaptop/gapbot.  Then
; move and redraw every pipe in use (pipe1).
; -----------------------------------------------------------------------------
pipes:  ld a,(newpipe)
        or a
        jr z,pp_mv
        xor a
        ld (newpipe),a
        ld hl,ptab
        ld de,4
        ld b,NPIPE
pp_f:   ld a,(hl)
        cp FREE
        jr z,pp_new
        add hl,de
        djnz pp_f
        jr pp_mv            ; no free slot (cannot happen 24 steps apart)
pp_new: ld (hl),64
        inc hl
        ld a,(gaptop)
        ld (hl),a
        inc hl
        ld a,(gapbot)
        ld (hl),a
pp_mv:  ld hl,ptab
        ld b,NPIPE
pp_lp:  push bc
        push hl
        call pipe1
        pop hl
        ld de,4
        add hl,de
        pop bc
        djnz pp_lp
        ret

; -----------------------------------------------------------------------------
; pipe1: move the pipe in slot HL one nibble left and draw it.  A pipe is 8
; nibbles wide: x .. x+7.  Its right edge passing the bat (x = 8, so x+7 = 15
; is just left of nibble 16) sets scored.  A pipe gone off the left (x < -8)
; frees its slot.  Each line gets 5 bytes from a pattern (body or cap, for
; even or odd x), clipped to the screen; the 5th byte rubs out the pixels
; the pipe has just left on its right.  The gap's lines are not touched.
; -----------------------------------------------------------------------------
pipe1:  ld a,(hl)
        cp FREE
        ret z
        dec a               ; one nibble left
        ld (hl),a
        cp BAT_NIB-8
        jr nz,p1_a
        ld a,1
        ld (scored),a       ; its right edge is past the bat
        ld a,(hl)
p1_a:   add a,8             ; x from -8 to 64 is on (or next to) the screen
        cp 73
        jr c,p1_on
        ld (hl),FREE        ; gone: free the slot
        ret
p1_on:  inc hl
        ld a,(hl)
        ld (pg1),a          ; gap top
        inc hl
        ld a,(hl)
        ld (pg2),a          ; gap bottom
        dec hl
        dec hl
        ld a,(hl)           ; A = x
        ld de,body_e        ; patterns for even x
        ld bc,cap_e
        bit 0,a
        jr z,p1_ev
        ld de,body_o        ; or odd x
        ld bc,cap_o
p1_ev:  ld (sbody),de
        ld (scap),bc
        sra a               ; A = first byte column = x / 2, rounded down
        ld c,5              ; C = bytes a line
        bit 7,a
        jr z,p1_r
        neg                 ; partly off the left: A = bytes cut off
        ld b,a
        ld a,c
        sub b
        ld c,a
        ld e,b              ; start that far into the patterns
        ld d,0
        ld hl,(sbody)
        add hl,de
        ld (sbody),hl
        ld hl,(scap)
        add hl,de
        ld (scap),hl
        xor a               ; and at column 0
p1_r:   ld (scol),a
        add a,c             ; partly off the right?
        sub 32
        jr c,p1_w
        ld b,a              ; B = bytes beyond column 31
        ld a,c
        sub b
        ret z               ; nothing on the screen yet
        ret c
        ld c,a
p1_w:   ld a,c
        ld (slen),a
        ld hl,(sbody)       ; body from the top down to the top cap
        ld (sptr),hl
        ld a,(pg1)
        sub 4
        ld b,a
        xor a
        call seg
        ld hl,(scap)        ; the cap above the gap
        ld (sptr),hl
        ld a,(pg1)
        sub 4
        ld b,4
        call seg
        ld a,(pg2)          ; the cap below it
        ld b,4
        call seg
        ld hl,(sbody)       ; body from below that cap to the ground
        ld (sptr),hl
        ld a,(pg2)
        add a,4
        ld c,a
        ld a,PF_H
        sub c
        ld b,a
        ld a,c
        jp seg

; -----------------------------------------------------------------------------
; seg: draw B lines (B > 0) of a pipe, from playfield line A down: on each
; line copy slen bytes of the pattern at sptr to byte column scol.
; -----------------------------------------------------------------------------
seg:    add a,PF_TOP
        call ln_addr
        ld a,(scol)
        or l
        ld l,a              ; HL = first byte to write
sg_lp:  push bc
        push hl
        ex de,hl            ; DE = screen
        ld hl,(sptr)
        ld bc,(slen)
        ldir
        pop hl
        call nxt_line
        pop bc
        djnz sg_lp
        ret

; -----------------------------------------------------------------------------
; ground: scroll the 8 ground lines 4 pixels left, feeding in each line's
; next pattern nibble at the right, then advance the pattern phase.
; -----------------------------------------------------------------------------
ground: ld a,(phase)
        ld e,a
        ld d,0
        ld hl,gpat
        add hl,de
        ex de,hl            ; DE -> this phase's nibble for ground line 0
        ld c,GR_TOP
gd_lp:  ld a,c
        call ln_addr
        ld a,l
        or 31               ; the line's last byte
        ld l,a
        ld a,(de)           ; nibble to feed in
        call rot_line
        inc de              ; same phase, next line: 4 bytes on
        inc de
        inc de
        inc de
        inc c
        ld a,c
        cp SC_END
        jr nz,gd_lp
        ld a,(phase)
        inc a
        and 3
        ld (phase),a
        ret

; -----------------------------------------------------------------------------
; rot_line: scroll one screen line 4 pixels left.  HL = its last byte (x 31),
; A's low nibble = the 4 pixels to bring in at the right.  RLD moves the
; byte's low nibble up, A's nibble into the low half, and the byte's old high
; nibble into A, which carries on into the byte to its left.  Unrolled:
; 32 x (RLD, DEC L), about 700 T-states a line.
; -----------------------------------------------------------------------------
rot_line:
        rept 31
        rld
        dec l
        endm
        rld
        ret

; -----------------------------------------------------------------------------
; bat_on: draw the bat at baty with this step's wing frame (the wings beat
; every two steps) and colour its cells yellow.  Returns C <> 0 if any of its
; pixels landed on something already on the screen (a pipe).
; -----------------------------------------------------------------------------
bat_on: ld a,(tick)
        and 2
        ld hl,spr_up
        jr z,bo_1
        ld hl,spr_dn
bo_1:   ld (oldspr),hl
        ex de,hl            ; DE = sprite
        ld a,(baty)
        ld (oldy),a
        call bat_xor        ; C = sprite AND screen, before drawing
        push bc
        ld a,(oldy)
        ld e,A_BAT
        call bat_attr
        ld a,1
        ld (drawn),a
        pop bc
        ret

; -----------------------------------------------------------------------------
; bat_off: if the bat is on the screen, XOR it away (which restores exactly
; what was under it) and give its cells back the sky colour.
; -----------------------------------------------------------------------------
bat_off: ld a,(drawn)
        or a
        ret z
        xor a
        ld (drawn),a
        ld de,(oldspr)
        ld a,(oldy)
        call bat_xor
        ld a,(oldy)
        ld e,A_SKY
        jp bat_attr

; -----------------------------------------------------------------------------
; dead: colour the bat red where it was last drawn (game over).
; -----------------------------------------------------------------------------
dead:   ld a,(oldy)
        ld e,A_DEAD
        jp bat_attr

; -----------------------------------------------------------------------------
; bat_xor: XOR the 16 x 8 sprite at DE onto the screen at byte column BAT_COL,
; playfield line A.  Returns C = the OR of (sprite AND screen) taken before
; each byte is changed: non-zero means the bat overlapped something.
; -----------------------------------------------------------------------------
bat_xor: add a,PF_TOP       ; playfield line -> screen line
        ld b,8
        ld c,0
bx_lp:  push af
        call ln_addr
        ld a,l
        or BAT_COL
        ld l,a
        ld a,(de)           ; left byte
        and (hl)
        or c
        ld c,a
        ld a,(de)
        xor (hl)
        ld (hl),a
        inc de
        inc l
        ld a,(de)           ; right byte
        and (hl)
        or c
        ld c,a
        ld a,(de)
        xor (hl)
        ld (hl),a
        inc de
        pop af
        inc a
        djnz bx_lp
        ret

; -----------------------------------------------------------------------------
; bat_attr: set the attribute E in the cells the bat covers when its top is at
; playfield line A: two columns, in the character row of its top line and of
; its bottom line (the same row when it is aligned).
; -----------------------------------------------------------------------------
bat_attr:
        add a,PF_TOP
        push af
        call ba_row         ; row of the top line
        pop af
        add a,7             ; row of the bottom line
ba_row: and $F8             ; A = screen line: row * 8
        ld l,a
        ld h,0
        add hl,hl
        add hl,hl           ; row * 32
        ld a,h
        add a,ATTRS/256
        ld h,a
        ld a,l
        or BAT_COL
        ld l,a
        ld (hl),e
        inc l
        ld (hl),e
        ret

; -----------------------------------------------------------------------------
; ln_addr: A = screen line 0-191 -> HL = address of its first byte.
; The Spectrum interleaves its screen: the address bits are
;   0 1 0 y7 y6 y2 y1 y0 | y5 y4 y3 x4 x3 x2 x1 x0
; Uses A and HL only, so BC and DE survive.
; -----------------------------------------------------------------------------
ln_addr:
        ld l,a
        and 7               ; y2-y0
        or $40
        ld h,a
        ld a,l
        rra
        rra
        rra
        and $18             ; y7 y6 -> bits 4, 3
        or h
        ld h,a
        ld a,l
        rla
        rla
        and $E0             ; y5-y3 -> bits 7-5
        ld l,a
        ret

; -----------------------------------------------------------------------------
; nxt_line: HL = a screen address -> the same column one line down.  Within
; a character row that is the next 256 bytes; at a row boundary, 32 bytes on
; and back 8 x 256 (unless that crosses into the next third of the screen).
; -----------------------------------------------------------------------------
nxt_line:
        inc h
        ld a,h
        and 7
        ret nz
        ld a,l
        add a,32
        ld l,a
        ret c
        ld a,h
        sub 8
        ld h,a
        ret

; -----------------------------------------------------------------------------
; fill: store A in BC bytes from HL (BC > 0), using LDIR.
; -----------------------------------------------------------------------------
fill:   ld (hl),a
        dec bc
        ld a,b
        or c
        ret z
        ld d,h
        ld e,l
        inc de
        ldir
        ret

; ---- data -------------------------------------------------------------------
; Sprites, ground and pipe patterns (generated by tools/gen_tables.py; the
; comments show the pixels).
spr_up:                         ; wings up: 16 x 8 pixels, 2 bytes a line
        defb $04,$20          ;      #    #     
        defb $87,$E1          ; #    ######    #
        defb $C5,$A3          ; ##   # ## #   ##
        defb $EF,$F7          ; ### ######## ###
        defb $7D,$BE          ;  ##### ## ##### 
        defb $3E,$7C          ;   #####  #####  
        defb $0F,$F0          ;     ########    
        defb $02,$40          ;       #  #      
spr_dn:                         ; wings down
        defb $04,$20          ;      #    #     
        defb $07,$E0          ;      ######     
        defb $05,$A0          ;      # ## #     
        defb $7F,$FE          ;  ############## 
        defb $FF,$FF          ; ################
        defb $ED,$B7          ; ### ## ## ## ###
        defb $CE,$73          ; ##  ###  ###  ##
        defb $83,$C1          ; #     ####     #
gpat:                           ; ground: 8 lines x 4 nibbles (one 16-pixel repeat)
        defb $F,$F,$F,$F      ; ################
        defb $8,$3,$8,$3      ; #     ###     ##
        defb $0,$E,$0,$E      ;     ###     ### 
        defb $3,$8,$3,$8      ;   ###     ###   
        defb $E,$0,$E,$0      ; ###     ###     
        defb $8,$3,$8,$3      ; #     ###     ##
        defb $0,$E,$0,$E      ;     ###     ### 
        defb $8,$8,$8,$8      ; #   #   #   #   
ginit:                          ; the same pattern as whole bytes, for the first fill
        defb $FF,$FF
        defb $83,$83
        defb $0E,$0E
        defb $38,$38
        defb $E0,$E0
        defb $83,$83
        defb $0E,$0E
        defb $88,$88
body_e:  defb $0F,$BF,$FF,$E0,$00   ; |    ##### #################             |  x even
body_o:  defb $00,$FB,$FF,$FE,$00   ; |        ##### #################         |  x odd
cap_e:  defb $7F,$BF,$FF,$FE,$00   ; | ######## #####################         |  x even
cap_o:  defb $07,$FB,$FF,$FF,$E0   ; |     ######## #####################     |  x odd

code_end:
