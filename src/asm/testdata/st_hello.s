start:	pea	message
	move.w	#9,-(sp)
	trap	#1
	addq.l	#6,sp
	lea	table,a0
	move.l	#table,d0
	move.w	#7,-(sp)
	trap	#1
	addq.l	#2,sp
	clr.w	-(sp)
	trap	#1
message:	dc.b	"Hello from Limoniter",13,10,0
	even
table:	dc.l	start,message,0
