package inter;

import treewalker.TreeWalker;


/*
 * If ist eine Unterklasse von Stmt. In der Instanzenvariable expr
 * wird der Ausdruck, in stmt die Anweisung abgelegt.
 */

public class If extends Stmt {
	Expr expr;
	Stmt stmt;

	public If(Expr x, Stmt s) {
		expr = x;
		stmt = s;
	}

	public Expr getExpr() {
		return expr;
	}

	public void setExpr(Expr expr) {
		this.expr = expr;
	}

	public Stmt getStmt() {
		return stmt;
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkIfNode(this, arg);
	}

}
