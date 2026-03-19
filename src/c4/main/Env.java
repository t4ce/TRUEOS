package main;

import java.util.*;
import lexer.*;
import inter.*;

public class Env {
	
	/*
	 * Dies ist die Klasse Env, die die Symboltabelle implementiert.
	 * Dargestellt wird sie als eine verkettete Liste von Hash-Tabellen,
	 * auf die jeweils mit einem Token als Schlüssel zugegeriffen wird.
	 * Als Werte hat man Objekte der Klasse Id.
	 * @author rp
	 */
	
	private Hashtable<Token,Id> table;
	protected Env prev;
	
	public Env(Env prevEnv) {
		table = new Hashtable<Token, Id>();
		prev = prevEnv;
	}
	
	// Eintragen in die Symboltabelle
	
	public Id put(Token w, Id id) {

		// 			Zum Testen der richtigen Funktion der Symboltabelle:
	
		// System.out.println("in table: " + id.getOp().toString() + "(" + id.getType() +")" + 
		//		"\t rel.Adr: " + id.getOffset());

		return table.put(w, id);
	}
	
	// Lesen aus der Symboltabelle
	
	public Id get (Token w) {
		for (Env e = this; e != null; e = e.prev ) {
			Id found = (Id) (e.table.get(w));
			if (found != null)
				return found;
		}
	return null;	
	}

}
