package main;

/*
 * Diese Klasse repräsentiert die Paare (trueLabel, falseLabel) und
 * fas Label nextLabel,die zur Übersetzung von Steuerstrukturen in 
 * der Beispiel - Programmiersprache benötigt werden.
 * 
 */

public class Labels {
	int first;
	int second;
	
	public Labels(int i, int j) {
		first = i;
		second = j;
	}
	
	public Labels(int i) {
		this(i,-1);		// als Nullwert
	}

	public int trueLabel() {
		return first;
	}
	
	public int falseLabel() {
		return second;
	}

	public int nextLabel() {
		return first;
	}

}
