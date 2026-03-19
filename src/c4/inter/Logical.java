package inter;

import lexer.*;

/*
 * Logical ist eine Unterklasse von Expr und beschreibt logische Ausdrücke. 
 * In den Instanzenvariablen expr1 und expr2 werden die beiden Operanden 
 * abgelegt
 */

public abstract class Logical extends Expr {
	Expr expr1, expr2;

	Logical(Token tok, Expr x1, Expr x2) {
		super(tok, null);
		expr1 = x1;
		expr2 = x2;
	}

	public Expr getExpr1() {
		return expr1;
	}

	public Expr getExpr2() {
		return expr2;
	}

	public void setExpr1(Expr expr1) {
		this.expr1 = expr1;
	}

	public void setExpr2(Expr expr2) {
		this.expr2 = expr2;
	}

}
