package inter;

import lexer.*;
import code.DreiAdrCode;
import code.Arith1OpCode;
import treewalker.TreeWalker;

/*
 * Unary ist eine Unterklasse von Op und beschreibt unäre arithmetische Ausdrücke. 
 * In der Instanzenvariablen expr ist der Operand abgelegt
 */

public class Unary extends Op {
	Expr expr;

	public Unary(Token tok, Expr x) { // behandelt unäres minus, für ! siehe Not
		super(tok);
		expr = x;
	}

	public Expr getExpr() {
		return expr;
	}

	public void setExpr(Expr expr) {
		this.expr = expr;
	}


	// für die Drei-Adress-Code Erzeugung
	public DreiAdrCode codeForValueTo(Id id) {
		return (new Arith1OpCode(id, this));
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkUnaryNode(this, arg);
	}

}
