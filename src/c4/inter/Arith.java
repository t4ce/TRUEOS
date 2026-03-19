package inter;
import treewalker.TreeWalker;
import code.DreiAdrCode;
import code.Arith2OpCode;

import lexer.*;

/*
 * Arith ist eine Unterklasse von Op und beschreibt arithmetische Ausdrücke. 
 * In den Instanzenvariablen expr1 und expr2 werden die beiden Operanden 
 * abgelegt
 */

public class Arith extends Op {
	Expr expr1, expr2;

	public Arith(Token tok, Expr x1, Expr x2) {
		super(tok);
		expr1 = x1;
		expr2 = x2;
	}
	
	// für TransformWalker:
	
	public Arith(Token tok, Expr x1, Expr x2, Type p) {
		super(tok);
		expr1 = x1;
		expr2 = x2;
		type = p;	
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

	// für die Drei-Adress-Code Erzeugung
	public DreiAdrCode codeForValueTo(Id id) {
		return (new Arith2OpCode(id, this));
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkArithNode(this, arg);
	}

}
