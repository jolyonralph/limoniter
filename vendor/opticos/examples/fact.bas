' Recursive functions
DEF FN fact(n)
  IF n <= 1 THEN RETURN 1
  RETURN n * fact(n - 1)
ENDFN
DEF FN sq(n) = n * n
FOR k = 0 TO 12 STEP 4
  PRINT k; "! = "; fact(k)
NEXT
PRINT "9 squared = "; sq(9)
