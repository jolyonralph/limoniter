; A small Amiga demo, as demo coders write it: the system's taken over, a copper list in chip memory puts
; colour bars down the screen, and the left mouse button ends it.
custom		equ	$dff000
dmacon		equ	$096
intena		equ	$09a
cop1lc		equ	$080
copjmp1		equ	$088
OpenLibrary	equ	-552
CloseLibrary	equ	-414

		section	main,code
start:		movem.l	d0-d7/a0-a6,-(sp)
		move.l	4.w,a6
		lea	gfxname(pc),a1
		moveq	#0,d0
		jsr	OpenLibrary(a6)
		move.l	d0,gfxbase
		beq.s	.out
		lea	custom,a5
		move.w	dmacon+2(a5),olddma
		move.w	#$7fff,intena(a5)
		move.w	#$7fff,dmacon(a5)
		move.l	#copper,cop1lc(a5)
		move.w	d0,copjmp1(a5)
		move.w	#$8280,dmacon(a5)
		lea	frames,a0
.wait:		addq.l	#1,(a0)
		btst	#6,$bfe001
		bne.s	.wait
		move.l	gfxbase,a1
		move.l	38(a1),cop1lc(a5)
		move.w	olddma,d0
		or.w	#$8000,d0
		move.w	d0,dmacon(a5)
		move.w	#$c000,intena(a5)
		move.l	4.w,a6
		jsr	CloseLibrary(a6)
.out:		movem.l	(sp)+,d0-d7/a0-a6
		moveq	#0,d0
		rts
gfxname:	dc.b	"graphics.library",0
		even

		section	vars,data
gfxbase:	dc.l	0
pointers:	dc.l	start,copper,frames,gfxname
		dc.w	1,2,3

		section	copper,data_c
copper:		dc.w	$0180,$0000
		dc.w	$2c07,$fffe,$0180,$0f00
		dc.w	$4c07,$fffe,$0180,$0ff0
		dc.w	$6c07,$fffe,$0180,$00f0
		dc.w	$8c07,$fffe,$0180,$00ff
		dc.w	$ffff,$fffe

		section	space,bss
olddma:		ds.w	1
frames:		ds.l	1
buffer:		ds.b	1001

		section	planes,bss_c
plane:		ds.b	40*256
