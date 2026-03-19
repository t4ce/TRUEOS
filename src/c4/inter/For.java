package inter;

import treewalker.TreeWalker;

public class For extends Stmt {

	/*
	 * For ist eine Unterklasse von Stmt. In der Instanzenvariable init_ass wird
	 * die Initialisierungs-Anweisung, in expr die Abbruchbedingung, in iter_ass
	 * die Iterationsanweisung und in stmt der Rumpf abgelegt.
	 */

	Expr expr;
	Assignment init_ass;
	Assignment iter_ass;
	Stmt stmt;

	public For(Assignment a1, Expr x, Assignment a2, Stmt s) {
		expr = x;
		stmt = s;
		init_ass = a1;
		iter_ass = a2;
	}

	public Expr getExpr() {
		return expr;
	}

	public void setExpr(Expr expr) {
		this.expr = expr;
	}

	public Assignment getInit_ass() {
		return init_ass;
	}

	public Assignment getIter_ass() {
		return iter_ass;
	}

	public Stmt getStmt() {
		return stmt;
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkForNode(this, arg);
	}

}
