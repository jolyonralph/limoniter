' Catching errors
DEF FN ratio(a, b)
  IF b = 0 THEN ERROR 200, "Nothing to divide by"
  RETURN a / b
ENDFN
DEF PROC test(a, b)
  TRY
    PRINT a; " / "; b; " = "; ratio(a, b)
  CATCH
    PRINT "Error "; ERR; ": "; ERR$
  ENDTRY
ENDPROC
test 10, 4
test 1, 0
TRY
  x = SQR -1
CATCH
  PRINT "Error "; ERR; ": "; ERR$
ENDTRY
