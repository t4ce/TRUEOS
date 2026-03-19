package inter;

import lexer.*;

/*
 * Op ist eine Unterklasse von Expr und beschreibt Operanden von Ausdrücken. 
 */

public abstract class Op extends Expr {

	public Op(Token tok) {
		super(tok, null);
	}

}
