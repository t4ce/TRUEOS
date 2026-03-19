package inter;

import treewalker.TreeWalker;

/*
 * Break ist eine Unterklasse von Stmt
 */

public class Break extends Stmt {
	Stmt stmt;

	public Break() {
	}

	public Stmt getStmt() {
		return stmt;
	}

	public void setStmt(Stmt stmt) {
		this.stmt = stmt;
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkBreakNode(this, arg);
	}

}
