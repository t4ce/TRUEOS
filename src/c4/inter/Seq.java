package inter;

import treewalker.TreeWalker;

/*
 * Seq ist eine Unterklasse von Stmt, die eine Folge von Anweisungen
 * beschreibt. In der Instanzenvariable stmt1 wird die erste, 
 * in stmt2 die restlichen Anweisungen abgelegt.
 */

public class Seq extends Stmt {
	Stmt stmt1;
	Stmt stmt2;

	public Seq(Stmt s1, Stmt s2) {
		stmt1 = s1;
		stmt2 = s2;
	}

	public Stmt getStmt1() {
		return stmt1;
	}

	public Stmt getStmt2() {
		return stmt2;
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkSeqNode(this, arg);
	}

}
