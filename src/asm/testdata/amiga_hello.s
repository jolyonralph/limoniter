; Writes a line to a file on the drive it was started from, through dos.library: how the tests know it ran.
		section	main,code
start:		move.l	4.w,a6
		lea	dosname(pc),a1
		moveq	#0,d0
		jsr	-552(a6)		; OpenLibrary
		move.l	d0,dosbase
		beq.s	.out
		move.l	d0,a6
		move.l	#filename,d1
		move.l	#1006,d2		; MODE_NEWFILE
		jsr	-30(a6)			; Open
		move.l	d0,handle
		beq.s	.close
		move.l	d0,d1
		move.l	#message,d2
		move.l	#length,d3
		jsr	-48(a6)			; Write
		move.l	handle,d1
		jsr	-36(a6)			; Close
.close:		move.l	a6,a1
		move.l	4.w,a6
		jsr	-414(a6)		; CloseLibrary
.out:		moveq	#0,d0
		rts
dosname:	dc.b	"dos.library",0

		section	strings,data
filename:	dc.b	"result.txt",0
message:	dc.b	"Hello from Limoniter",10
length		equ	*-message

		section	vars,bss
dosbase:	ds.l	1
handle:		ds.l	1
