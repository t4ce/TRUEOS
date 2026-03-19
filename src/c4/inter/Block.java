package inter;

import treewalker.TreeWalker;

/*
 * Block ist eine Unterklasse von Stmt. In der Instanzenvariable stmts
 * werden die Anweisungen des Blocks abgelegt.
 */

public class Block extends Stmt {
	Stmt stmts;

	public Block(Stmt s) {
		stmts = s;
	}

	public Stmt getStmts() {
		return stmts;
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkBlockNode(this, arg);
	}

}
