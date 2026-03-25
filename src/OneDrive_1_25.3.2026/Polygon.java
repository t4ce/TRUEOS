package com.mycompany.SwingOpenGL;

import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;
import java.util.Random;

// i found a lot of good information on hs-flensburg.de, i did not look at the provided code 
// so this implementation is a completely diffrent approach, it's mainly ment as a sidekick model to the opengl render class
// https://www.inf.hs-flensburg.de/lang/algorithmen/geo/graham.htm

// Polygon Class
public class Polygon{
    // Head of the linked List
    public Vertex startVertex;
    // Number of Polygon-Vertices
    public int vertexCount;
    // We assume bottom left to be the starPoint
    public Vertex starPoint;
    // Sub Polygon that represents the convex hull
    public Polygon convexHull;
    
    // constructor for Polygons with n vertices
    public Polygon(int n){
        this.vertexCount = n;
        startVertex = new Vertex();
        Vertex currentVertex = startVertex;
        for (int i = 0; i < vertexCount - 1; i++) {
            Vertex newVertex = new Vertex();
            currentVertex.nextVertex = newVertex;
            currentVertex = newVertex;
        }
        currentVertex.nextVertex = startVertex;
        findStarPoint();
        sortByAngle();
        calculateConvexHull();
    }
    public Polygon(){
        vertexCount = 0;
    }
    public Polygon copyPolygon(){
        Polygon pNew = new Polygon();
        pNew.startVertex = new Vertex();
        pNew.startVertex.x = startVertex.x;
        pNew.startVertex.y = startVertex.y;   
        pNew.vertexCount++;
        Vertex nextNew = pNew.startVertex;
        Vertex next = startVertex.nextVertex;
        do{
            nextNew.nextVertex = new Vertex();
            nextNew.nextVertex.x = next.x;
            nextNew.nextVertex.y = next.y;
            nextNew = nextNew.nextVertex;  
            pNew.vertexCount++;
            next = next.nextVertex;
        }while(startVertex != next);
        nextNew.nextVertex = pNew.startVertex;
        return pNew;
    }
    
    // Graham-Scan-Algorithmus
    public void calculateConvexHull(){
        convexHull = copyPolygon();
        Vertex next = convexHull.startVertex.nextVertex;
        do{
            if (isKonkave(next)){
                // remove the konkave vertex
                Vertex predecessor = getPredecessor(next);
                Vertex successor = getSuccessor(next);
                predecessor.nextVertex = successor;
                // decrement the vertexCount accordingly
                convexHull.vertexCount--;
                next = predecessor;
            }else{
                next = next.nextVertex;
            }
        }while(convexHull.startVertex != next);        
     }
    
    Vertex getPredecessor(Vertex v){
        Vertex next = v.nextVertex;
        do{
            next = next.nextVertex;
        }while(v != next.nextVertex);
        return next;
    }
    
    Vertex getSuccessor(Vertex v){
        return v.nextVertex;
    }
    
    @Override
    public String toString(){
        String toStr = "";
        Vertex next = startVertex;
        do{            
            toStr += next.toString();
            next = next.nextVertex;
        }while(startVertex != next);          
        return toStr;
    }
    
    // Diese Aufgabe kann man durch Bestimmen aller relevanten Winkel lösen, 
    // oder einfacher durch die Berechnung einer Determinante {\displaystyle T(A,B,C)}T(A, B, C), 
    // diese liefert das gewünschte Ergebnis mit weniger Rechenaufwand (fünf Subtraktionen, zwei Multiplikationen) und genauer.
    public boolean isKonkave(Vertex v){
        // T(A,B,C) = (xb-xa)(yc-ya)-(xc-xa)(yb-ya)
        // T(P,V,S) = (xV-xP)(yS-yP)-(xS-xP)(yV-yP)
        Vertex p = getPredecessor(v);
        Vertex s = getSuccessor(v);
        // T<0 <=> isKonkave (or on the Line anyways in case of T=0) 
        return (v.x - p.x) * (s.y - p.y) - (s.x - p.x) * (v.y - p.y) <= 0;
    }
    
    public void sortByAngle(){
        List<Vertex> vl = new ArrayList<>();
        
        // calculate angles
        Vertex next = startVertex;
        do{            
            next.angle = Math.atan2(next.x - starPoint.x, next.y - starPoint.y);//Math.atan((starPoint.y - next.y) / (starPoint.x - next.x));
            vl.add(next);
            next = next.nextVertex;
        }while(startVertex != next);  
        
        // order list by angle low to high
        Collections.sort(vl, new Comparator<Vertex>(){
            @Override
            public int compare(Vertex a, Vertex b){
                // smaller
                if(a.angle < b.angle)
                    return 1;
                // bigger
                if(a.angle > b.angle)
                    return -1;
                // equal
                return 0;
            }
        });
        
        // update Polygon
        // the starPoint is now our Polygon startVertex
        startVertex = starPoint;
        Vertex currentVertex = startVertex;
        for (int i = 0; i < vertexCount; i++) {
            // from angle low to high, append the vertices to the Polygon
            Vertex newVertex = vl.get(i);
            // the starpoint is already head/startVertex of the Polygon, we skip it ( its angle to itself is 0, we must exclude it anyways)
            if(starPoint != newVertex){
                currentVertex.nextVertex = newVertex;
                currentVertex = newVertex;
            }
        }
        // connect the last Vertex (highest angle) back to our starPoint aka starVertex
        currentVertex.nextVertex = startVertex;
    }
 
    // basicly bottom left
    private void findStarPoint(){
        Vertex start = this.startVertex;
        Vertex next = start.nextVertex;
        starPoint = start;
        double low = start.y;
        do{
            if (next.y < low){
                low = next.y;
                starPoint = next;
            }  
            next = next.nextVertex;
        }while(start != next);  
    }
    
    // Polygon Vertex Class
    public class Vertex{
        Random r = new Random();
        // 2 random doubles as cord's between -50 <-> +50
        public double x = 100 * r.nextDouble() - 50;
        public double y = 100 * r.nextDouble() - 50;
        public Vertex nextVertex;
        // angle in respect to the starPoint, its main usage is to sort the Vertices 
        double angle;
        @Override
        public String toString(){
            return String.format("(%.0f,%.0f)", x, y);
        }
    }
}