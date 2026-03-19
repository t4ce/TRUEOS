package inter;

import treewalker.TreeWalker;


/*
 * Do ist eine Unterklasse von Stmt. In der Instanzenvariable expr
 * wird der Ausdruck, in stmt die Anweisung abgelegt.
 */

public class Do extends Stmt {
	Expr expr;
	Stmt stmt;

	public Do(Stmt s, Expr x) {
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
		return walker.walkDoNode(this, arg);
	}

}
