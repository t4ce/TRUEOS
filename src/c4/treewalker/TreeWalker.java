package treewalker;

import inter.Access;
import inter.And;
import inter.Arith;
import inter.AssignElem;
import inter.AssignId;
import inter.AssignStmt;
import inter.Block;
import inter.Break;
import inter.Constant;
import inter.Do;
import inter.Else;
import inter.EmptyStmt;
import inter.For;
import inter.Id;
import inter.If;
import inter.Node;
import inter.Not;
import inter.Or;
import inter.Program;
import inter.Rel;
import inter.Seq;
import inter.Unary;
import inter.While;

/*
 * Diese Klasse definiert einen abstrakten TreeWalker, der den Syntaxbaum durchläuft
 * und durch double Dispatch die jeweilig zum Knotentyp passenden Methoden auswählt.
 * In den abgeleiteten Unterklassen müssen für jeden Knotentyp die zugehörigen Methoden
 * implementiert werden. 
 */

public abstract class TreeWalker <R, P>{
	
	public R walk (Node node, P arg) {
		return node.walk(this, arg);
	}

	/*
	 * Hier muss für jeden Knotentyp eine abstrakte Methode eingetragen werden.
	 * In jeder Knoten-Klasse XYZ muss eine Methode walk der Form
	 * 
	 * 	public <ReturnType, ArgumentType> ReturnType 
	 * 			walk(TreeWalker<ReturnType, ArgumentType> walker, ArgumentType arg) {
	 * 					return walker.walkXYZNode(this, arg);
	 * 
	 * definiert werden.
	 */
	
	public abstract R walkAccessNode(Access node, P arg);
	public abstract R walkAndNode(And node, P arg);
	public abstract R walkArithNode(Arith node, P arg);
	public abstract R walkAssignElemNode(AssignElem node, P arg);
	public abstract R walkAssignIdNode(AssignId node, P arg);
	public abstract R walkAssignStmtNode(AssignStmt node, P arg);
	public abstract R walkBlockNode(Block node, P arg);
	public abstract R walkBreakNode(Break node, P arg);
	public abstract R walkConstantNode(Constant node, P arg); 
	public abstract R walkDoNode(Do node, P arg);
	public abstract R walkElseNode(Else node, P arg);
	public abstract R walkEmptyStmtNode(EmptyStmt node, P arg); 
	public abstract R walkForNode(For node, P arg);
	public abstract R walkIdNode(Id node, P arg);
	public abstract R walkIfNode(If node, P arg);
	public abstract R walkNotNode(Not node, P arg);
	public abstract R walkOrNode(Or node, P arg);
	public abstract R walkProgramNode(Program node, P arg);
	public abstract R walkRelNode(Rel node, P arg);
	public abstract R walkSeqNode(Seq node, P arg);
	public abstract R walkUnaryNode(Unary node, P arg);
	public abstract R walkWhileNode(While node, P arg);

}
