package inter;
import treewalker.TreeWalker;
/*
 * Die Unterklasse AssignStmt von Stmt definiert beschreibt Wertzuweisungen, 
 * die als Anweisungen auftreten. 
 */

public class AssignStmt extends Stmt {
	Assignment assign;

	public AssignStmt(Assignment x) {
		assign = x;
	}

	public Assignment getAssign() {
		return assign;
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkAssignStmtNode(this, arg);
	}

}
