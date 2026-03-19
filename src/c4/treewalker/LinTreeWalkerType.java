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
import inter.Not;
import inter.Or;
import inter.Program;
import inter.Rel;
import inter.Seq;
import inter.Unary;
import inter.While;

/*
 * Dies ist eine Unterklasse der Klasse TreeWalker, die den Syntaxbaum 
 * durchläuft und eine graphische Darstellung des Baums erzeugt.
 * Jeder Knoten des Syntaxbaums wird durch eine Zeile in der Ausgabe 
 * repräsentiert, in der zunächst eine Zeichenkette preStr die Tiefe
 * darstellt und dann eine Bezeichnung des Knotens folgt.
 * 
 * Jede Methode gibt null als Rückgabewert zurück.
 */

public class LinTreeWalkerType extends TreeWalker<Void, String> {
	static String indent = "|  ";
	
	public Void walkAccessNode(Access node, String preStr) {
		System.out.println(preStr + "Access" + "(" + node.getType() +")");
		walk(node.getArray(), preStr + indent);
		walk(node.getIndex(), preStr + indent);
		return null;
	}
	
	public Void walkAndNode(And node, String preStr) {
		System.out.println(preStr + "And" + "(" + node.getType() +")");
		walk (node.getExpr1(), preStr + indent);
		walk( node.getExpr2(), preStr + indent);
		return null;
	}	
	
	public Void walkArithNode(Arith node, String preStr) {
		System.out.println(preStr + "Arith (" + node.getOp().toString() + ")" + "(" + node.getType() +")");
		walk(node.getExpr1(),preStr + indent);
		walk(node.getExpr2(), preStr + indent);
		return null;
	}
	public Void walkAssignElemNode(AssignElem node, String preStr) {
		System.out.println(preStr + "AssignElem");
		walk(node.getAcc(), preStr + indent);
		walk(node.getExpr(), preStr + indent);
		return null;
	}

	public Void walkAssignIdNode(AssignId node, String preStr) {
		System.out.println(preStr + "AssignId");
		walk(node.getIdent(), preStr + indent);
		walk(node.getExpr(), preStr + indent);
		return null;
	}
	
	public Void walkAssignStmtNode(AssignStmt node, String preStr) {
		System.out.println(preStr + "AssignStmt");
		walk(node.getAssign(), preStr + indent);
		return null;
	}
	
	public Void walkBlockNode(Block node, String preStr) {
		System.out.println(preStr + "Block");
		walk(node.getStmts(), preStr + indent);
		return null;
	}
	
	public Void walkBreakNode(Break node, String preStr) {
		System.out.println(preStr + "Break");
		return null;
	}	

	public Void walkConstantNode(Constant node, String preStr) {
		System.out.println(preStr + "Constant (" + node.getOp().toString() + ")" + "(" + node.getType() +")");
		return null;
	}
	
	public Void walkDoNode(Do node, String preStr) {
		System.out.println(preStr + "Do");
		walk(node.getStmt(), preStr + indent);
		walk(node.getExpr(), preStr + indent);
		return null;
	}
	
	public Void walkElseNode(Else node, String preStr) {
		System.out.println(preStr + "IfElse");
		walk(node.getExpr(), preStr + indent);
		walk(node.getStmt1(), preStr + indent);
		walk(node.getStmt2(), preStr + indent);
		return null;
	}

	public Void walkEmptyStmtNode(EmptyStmt node, String preStr) {
		System.out.println(preStr + "EmptyStmt");
			return null; 
	}
	
	public Void walkForNode(For node, String preStr) {
		System.out.println(preStr + "For");
		walk(node.getInit_ass(), preStr + indent);
		walk(node.getExpr(), preStr + indent);
		walk(node.getIter_ass(), preStr + indent);
		walk(node.getStmt(), preStr + indent);
		return null;
	}
	
	public Void walkIdNode(Id node, String preStr) {
		System.out.println(preStr + "Id (" + node.getOp().toString() + ")" + "(" + node.getType() +")");
		return null;
	}
	
	public Void walkIfNode(If node, String preStr) {
		System.out.println(preStr + "If");
		walk(node.getExpr(), preStr + indent);
		walk(node.getStmt(), preStr + indent);
		return null;
	}

	public Void walkNotNode(Not node, String preStr) {
		System.out.println(preStr + "Not" + "(" + node.getType() +")");
		walk(node.getExpr1(), preStr + indent);
		return null;
	}
	
	public Void walkOrNode(Or node, String preStr) {
		System.out.println(preStr + "Or" + "(" + node.getType() +")");
		walk(node.getExpr1(), preStr + indent);
		walk(node.getExpr2(), preStr + indent);
		return null;
	}
	
	public Void walkProgramNode(Program node, String preStr) {
		System.out.println(preStr + "Program");
		walk(node.getBlock(), preStr);
		return null;
	}
	
	public Void walkRelNode(Rel node, String preStr) {
		System.out.println(preStr + "Rel (" + node.getOp().toString() + ")" + "(" + node.getType() +")");
		walk(node.getExpr1(), preStr + indent);
		walk(node.getExpr2(), preStr + indent);
		return null;
	}
	
	public Void walkSeqNode(Seq node, String preStr) {
		System.out.println(preStr + "Seq");
		walk(node.getStmt1(), preStr +indent);
		walk(node.getStmt2(), preStr);
		return null;
	}

	public Void walkUnaryNode(Unary node, String preStr) {
		System.out.println(preStr + "Unary(" + node.getOp().toString()  + ")" + "(" + node.getType() +")");
		walk (node.getExpr(), preStr + indent);
		return null;
	}
	
	public Void walkWhileNode(While node, String preStr) {
		System.out.println(preStr + "While");
		walk(node.getExpr(), preStr + indent);
		walk(node.getStmt(), preStr + indent);
		return null;
	}
}
